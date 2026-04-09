# Context Engine - Especificação Técnica Precisa
*Sistema de análise de contexto baseado em AST e graph traversal*

## 🎯 **Objetivo e Escopo**

### Definição Clara
**Context Engine** é o componente que analisa projetos de código e fornece contexto relevante para o LLM, permitindo respostas precisas e contextualmente apropriadas.

### Responsabilidades Específicas
1. **Análise de Projeto**: Parse AST + construção de graph de dependências
2. **Busca Contextual**: Encontrar arquivos/código relevantes para queries
3. **Pattern Detection**: Identificar frameworks e padrões arquiteturais
4. **Cache Management**: Otimizar performance via caching inteligente

### Não-Responsabilidades
- Não executa código ou comandos
- Não modifica arquivos (apenas lê)
- Não faz inferências sobre business logic
- Não integra com LLM (apenas prepara contexto)

## 🏗️ **Arquitetura Técnica**

### Componentes Core
```python
ContextEngine
├─ ProjectAnalyzer          # AST parsing + graph construction
├─ ContextSearcher          # Relevance-based file search
├─ FrameworkDetector        # Pattern recognition
├─ ContextCache             # SQLite-based caching
└─ ContextFormatter         # Output formatting for LLM
```

### Data Flow
```
1. Project Path → ProjectAnalyzer → AST + Graph
2. User Query → ContextSearcher → Relevant Files
3. Graph + Files → FrameworkDetector → Patterns
4. All Data → ContextFormatter → LLM-Ready Context
```

## 📊 **Performance Specifications**

### Targets Mensuráveis
```
Analysis Time:
├─ Small projects (<50 files): 1-3s
├─ Medium projects (50-200 files): 3-8s
├─ Large projects (200-500 files): 8-15s
└─ Very large projects (>500 files): 15-30s

Memory Usage:
├─ Base overhead: 50MB
├─ Per file analyzed: 100KB
├─ Graph storage: 200KB per 100 files
└─ Cache overhead: 20MB

Accuracy Targets:
├─ Framework detection: >90%
├─ Relevant file finding: >70%
├─ Pattern recognition: >80%
└─ Context usefulness: >75% (user feedback)
```

### Cache Performance
```
Cache Hit Rate: 60-80%
Cache Response Time: <200ms
Cache Size Limit: 500MB
Cache TTL: 24 hours (configurable)
```

## 🔧 **Implementação Detalhada**

### 1. ProjectAnalyzer
```python
class ProjectAnalyzer:
    """Análise de projeto via AST parsing"""
    
    def __init__(self):
        self.supported_languages = {
            'python': 'tree-sitter-python',
            'javascript': 'tree-sitter-javascript',
            'typescript': 'tree-sitter-typescript',
            'go': 'tree-sitter-go',
            'rust': 'tree-sitter-rust'
        }
        self.parsers = {}
        self.file_filters = [
            '*.py', '*.js', '*.ts', '*.go', '*.rs',
            '*.json', '*.yaml', '*.yml', '*.toml',
            '*.md', '*.txt', '*.sql'
        ]
    
    async def analyze_project(self, project_path: str) -> ProjectGraph:
        """Análise completa do projeto"""
        
        # 1. Descobrir arquivos relevantes
        relevant_files = self.discover_files(project_path)
        
        # 2. Parse AST para cada arquivo
        ast_data = {}
        for file_path in relevant_files:
            try:
                ast_data[file_path] = await self.parse_file(file_path)
            except Exception as e:
                logging.warning(f"Failed to parse {file_path}: {e}")
                continue
        
        # 3. Construir graph de dependências
        graph = self.build_dependency_graph(ast_data)
        
        # 4. Adicionar metadata útil
        graph.metadata = {
            'total_files': len(relevant_files),
            'parsed_files': len(ast_data),
            'analysis_timestamp': datetime.now(),
            'project_path': project_path
        }
        
        return graph
    
    def discover_files(self, project_path: str) -> List[str]:
        """Descobrir arquivos relevantes para análise"""
        
        relevant_files = []
        ignore_patterns = self.get_ignore_patterns(project_path)
        
        for pattern in self.file_filters:
            files = glob.glob(os.path.join(project_path, '**', pattern), recursive=True)
            for file_path in files:
                if not self.should_ignore_file(file_path, ignore_patterns):
                    relevant_files.append(file_path)
        
        # Limitar número de arquivos para performance
        if len(relevant_files) > 1000:
            relevant_files = self.prioritize_files(relevant_files)[:1000]
        
        return relevant_files
    
    async def parse_file(self, file_path: str) -> Dict:
        """Parse AST de um arquivo específico"""
        
        language = self.detect_language(file_path)
        if language not in self.supported_languages:
            return self.parse_as_text(file_path)
        
        parser = self.get_parser(language)
        
        try:
            with open(file_path, 'r', encoding='utf-8') as f:
                content = f.read()
        except UnicodeDecodeError:
            return {'error': 'encoding_error', 'path': file_path}
        
        tree = parser.parse(bytes(content, 'utf-8'))
        
        return {
            'path': file_path,
            'language': language,
            'ast': tree,
            'content': content,
            'size': len(content),
            'lines': content.count('\n') + 1,
            'functions': self.extract_functions(tree, language),
            'classes': self.extract_classes(tree, language),
            'imports': self.extract_imports(tree, language),
            'exports': self.extract_exports(tree, language)
        }
    
    def build_dependency_graph(self, ast_data: Dict) -> ProjectGraph:
        """Construir graph de dependências"""
        
        graph = ProjectGraph()
        
        # Adicionar nodes (arquivos)
        for file_path, ast_info in ast_data.items():
            if 'error' in ast_info:
                continue
                
            graph.add_node(
                file_path,
                language=ast_info['language'],
                functions=ast_info['functions'],
                classes=ast_info['classes'],
                size=ast_info['size'],
                lines=ast_info['lines']
            )
        
        # Adicionar edges (dependências)
        for file_path, ast_info in ast_data.items():
            if 'error' in ast_info:
                continue
                
            for import_info in ast_info['imports']:
                target_file = self.resolve_import(import_info, file_path, ast_data)
                if target_file:
                    graph.add_edge(file_path, target_file, type='import')
        
        return graph
```

### 2. ContextSearcher
```python
class ContextSearcher:
    """Busca por contexto relevante baseado em query"""
    
    def __init__(self):
        self.keyword_extractor = KeywordExtractor()
        self.relevance_calculator = RelevanceCalculator()
        self.max_context_files = 8
        self.max_context_size = 50000  # ~50KB context limit
    
    async def search_relevant_context(
        self, 
        query: str, 
        project_graph: ProjectGraph,
        search_mode: str = 'balanced'
    ) -> List[ContextFile]:
        """Buscar contexto relevante para a query"""
        
        # 1. Extrair keywords da query
        keywords = self.keyword_extractor.extract(query)
        
        # 2. Calcular relevância para cada arquivo
        file_relevance = {}
        for file_path in project_graph.nodes:
            relevance = self.relevance_calculator.calculate_relevance(
                file_path, keywords, project_graph, query
            )
            if relevance > 0.3:  # Threshold mínimo
                file_relevance[file_path] = relevance
        
        # 3. Ranking e seleção
        sorted_files = sorted(
            file_relevance.items(), 
            key=lambda x: x[1], 
            reverse=True
        )
        
        # 4. Selecionar arquivos até limite de contexto
        selected_files = []
        total_size = 0
        
        for file_path, relevance in sorted_files:
            if len(selected_files) >= self.max_context_files:
                break
            
            file_size = project_graph.nodes[file_path].get('size', 0)
            if total_size + file_size > self.max_context_size:
                break
            
            selected_files.append(ContextFile(
                path=file_path,
                relevance=relevance,
                content=self.get_file_content(file_path),
                metadata=project_graph.nodes[file_path]
            ))
            total_size += file_size
        
        return selected_files
```

### 3. FrameworkDetector
```python
class FrameworkDetector:
    """Detecção de frameworks e padrões arquiteturais"""
    
    def __init__(self):
        self.framework_patterns = {
            'fastapi': FastAPIPatternDetector(),
            'django': DjangoPatternDetector(),
            'flask': FlaskPatternDetector(),
            'react': ReactPatternDetector(),
            'nextjs': NextJSPatternDetector(),
            'vue': VuePatternDetector(),
            'express': ExpressPatternDetector()
        }
    
    def detect_framework(self, project_graph: ProjectGraph) -> FrameworkInfo:
        """Detectar framework principal do projeto"""
        
        detection_results = {}
        
        for framework_name, detector in self.framework_patterns.items():
            confidence = detector.detect_confidence(project_graph)
            if confidence > 0.5:
                detection_results[framework_name] = {
                    'confidence': confidence,
                    'patterns': detector.get_detected_patterns(),
                    'conventions': detector.get_conventions(),
                    'recommendations': detector.get_recommendations()
                }
        
        # Selecionar framework com maior confidence
        if detection_results:
            primary_framework = max(detection_results.items(), key=lambda x: x[1]['confidence'])
            return FrameworkInfo(
                name=primary_framework[0],
                confidence=primary_framework[1]['confidence'],
                patterns=primary_framework[1]['patterns'],
                conventions=primary_framework[1]['conventions'],
                all_detected=detection_results
            )
        
        return FrameworkInfo(name='unknown', confidence=0.0)

class FastAPIPatternDetector:
    """Detector específico para FastAPI"""
    
    def detect_confidence(self, project_graph: ProjectGraph) -> float:
        """Calcular confidence de detecção do FastAPI"""
        
        indicators = {
            'fastapi_import': 0.4,
            'app_instance': 0.3,
            'route_decorators': 0.2,
            'pydantic_models': 0.1
        }
        
        confidence = 0.0
        
        for file_path, node_data in project_graph.nodes.items():
            if not file_path.endswith('.py'):
                continue
            
            # Verificar import do FastAPI
            if any('fastapi' in imp.lower() for imp in node_data.get('imports', [])):
                confidence += indicators['fastapi_import']
            
            # Verificar instância do app
            if any('FastAPI' in func for func in node_data.get('functions', [])):
                confidence += indicators['app_instance']
            
            # Verificar decoradores de rota
            functions = node_data.get('functions', [])
            route_decorators = ['@app.get', '@app.post', '@app.put', '@app.delete']
            if any(any(dec in func for dec in route_decorators) for func in functions):
                confidence += indicators['route_decorators']
        
        return min(confidence, 1.0)
    
    def get_detected_patterns(self) -> List[str]:
        """Padrões detectados específicos do FastAPI"""
        return [
            'REST API endpoints',
            'Dependency injection',
            'Pydantic models',
            'Async handlers',
            'OpenAPI integration'
        ]
    
    def get_conventions(self) -> Dict[str, str]:
        """Convenções do FastAPI"""
        return {
            'app_instance': 'app = FastAPI()',
            'route_prefix': '/api/v1',
            'model_location': 'models/',
            'schema_location': 'schemas/',
            'router_location': 'routers/'
        }
```

### 4. ContextCache
```python
class ContextCache:
    """Cache inteligente para análises de contexto"""
    
    def __init__(self, cache_dir: str = '.ai-assistant/cache'):
        self.cache_dir = Path(cache_dir)
        self.cache_dir.mkdir(parents=True, exist_ok=True)
        self.db_path = self.cache_dir / 'context_cache.db'
        self.max_cache_size = 500 * 1024 * 1024  # 500MB
        self.default_ttl = 24 * 3600  # 24 hours
    
    async def get_cached_analysis(self, project_path: str) -> Optional[ProjectGraph]:
        """Recuperar análise cached"""
        
        cache_key = self.generate_cache_key(project_path)
        
        async with aiosqlite.connect(self.db_path) as db:
            cursor = await db.execute(
                "SELECT data, timestamp FROM context_cache WHERE key = ?",
                (cache_key,)
            )
            row = await cursor.fetchone()
            
            if row:
                data, timestamp = row
                # Verificar se não expirou
                if time.time() - timestamp < self.default_ttl:
                    return pickle.loads(data)
                else:
                    # Remover entrada expirada
                    await db.execute("DELETE FROM context_cache WHERE key = ?", (cache_key,))
                    await db.commit()
        
        return None
    
    async def cache_analysis(self, project_path: str, analysis: ProjectGraph):
        """Armazenar análise no cache"""
        
        cache_key = self.generate_cache_key(project_path)
        data = pickle.dumps(analysis)
        timestamp = time.time()
        
        # Verificar limite de tamanho
        if len(data) > 50 * 1024 * 1024:  # 50MB por item
            logging.warning(f"Analysis too large to cache: {len(data)} bytes")
            return
        
        async with aiosqlite.connect(self.db_path) as db:
            await db.execute(
                "INSERT OR REPLACE INTO context_cache (key, data, timestamp, size) VALUES (?, ?, ?, ?)",
                (cache_key, data, timestamp, len(data))
            )
            await db.commit()
        
        # Cleanup se necessário
        await self.cleanup_cache_if_needed()
    
    def generate_cache_key(self, project_path: str) -> str:
        """Gerar chave única para o projeto"""
        
        # Incluir hash dos timestamps dos arquivos para invalidação automática
        file_hashes = []
        for root, dirs, files in os.walk(project_path):
            # Ignorar diretórios comuns
            dirs[:] = [d for d in dirs if not d.startswith('.') and d not in ['node_modules', '__pycache__']]
            
            for file in files:
                if file.endswith(('.py', '.js', '.ts', '.json', '.yaml', '.yml')):
                    file_path = os.path.join(root, file)
                    try:
                        mtime = os.path.getmtime(file_path)
                        file_hashes.append(f"{file}:{mtime}")
                    except OSError:
                        continue
        
        # Usar hash dos file hashes + project path
        content = f"{project_path}:{':'.join(sorted(file_hashes))}"
        return hashlib.sha256(content.encode()).hexdigest()
```

## 🧪 **Testing Strategy**

### Unit Tests
```python
# test_context_engine.py
class TestContextEngine:
    """Testes unitários para Context Engine"""
    
    def test_project_analysis_fastapi(self):
        """Testar análise de projeto FastAPI"""
        engine = ContextEngine()
        result = engine.analyze_project('fixtures/fastapi_project')
        
        assert result.framework.name == 'fastapi'
        assert result.framework.confidence > 0.8
        assert len(result.files) > 0
    
    def test_context_search_relevance(self):
        """Testar busca de contexto relevante"""
        engine = ContextEngine()
        graph = self.load_test_graph()
        
        results = engine.search_relevant_context(
            "add user authentication", 
            graph
        )
        
        assert len(results) > 0
        assert any('user' in r.path.lower() for r in results)
        assert any('auth' in r.path.lower() for r in results)
    
    def test_cache_performance(self):
        """Testar performance do cache"""
        cache = ContextCache()
        
        # Primeira análise (cache miss)
        start = time.time()
        result1 = cache.get_cached_analysis('test_project')
        time1 = time.time() - start
        
        # Segunda análise (cache hit)
        start = time.time()
        result2 = cache.get_cached_analysis('test_project')
        time2 = time.time() - start
        
        assert time2 < time1 * 0.1  # Cache deve ser 10x mais rápido
```

### Integration Tests
```python
# test_integration.py
class TestIntegration:
    """Testes de integração end-to-end"""
    
    def test_real_project_analysis(self):
        """Testar análise de projeto real"""
        # Usar projeto FastAPI de exemplo
        project_path = 'fixtures/real_fastapi_project'
        
        engine = ContextEngine()
        result = engine.analyze_project(project_path)
        
        # Verificar resultados
        assert result.framework.name == 'fastapi'
        assert len(result.files) > 10
        assert result.patterns.includes('REST API endpoints')
    
    def test_performance_benchmarks(self):
        """Testar benchmarks de performance"""
        projects = [
            ('small', 'fixtures/small_project', 50),
            ('medium', 'fixtures/medium_project', 200),
            ('large', 'fixtures/large_project', 500)
        ]
        
        engine = ContextEngine()
        
        for size, path, file_count in projects:
            start = time.time()
            result = engine.analyze_project(path)
            analysis_time = time.time() - start
            
            # Verificar targets de performance
            if size == 'small':
                assert analysis_time < 3.0
            elif size == 'medium':
                assert analysis_time < 8.0
            elif size == 'large':
                assert analysis_time < 15.0
```

## 📈 **Monitoring e Métricas**

### Métricas Coletadas
```python
class ContextEngineMetrics:
    """Coleta de métricas do Context Engine"""
    
    def __init__(self):
        self.metrics = {
            'analysis_times': [],
            'cache_hit_rates': [],
            'framework_detection_accuracy': [],
            'context_relevance_scores': [],
            'memory_usage': []
        }
    
    def record_analysis(self, analysis_time: float, cache_hit: bool):
        """Registrar métricas de análise"""
        self.metrics['analysis_times'].append(analysis_time)
        self.metrics['cache_hit_rates'].append(1 if cache_hit else 0)
    
    def get_performance_summary(self) -> Dict:
        """Resumo de performance"""
        return {
            'avg_analysis_time': np.mean(self.metrics['analysis_times']),
            'p95_analysis_time': np.percentile(self.metrics['analysis_times'], 95),
            'cache_hit_rate': np.mean(self.metrics['cache_hit_rates']),
            'avg_memory_usage': np.mean(self.metrics['memory_usage'])
        }
```

### Alertas de Performance
```python
class PerformanceMonitor:
    """Monitor de performance com alertas"""
    
    def __init__(self):
        self.thresholds = {
            'analysis_time_p95': 15.0,  # 15s
            'cache_hit_rate_min': 0.6,   # 60%
            'memory_usage_max': 500,     # 500MB
        }
    
    def check_performance(self, metrics: Dict) -> List[str]:
        """Verificar thresholds e gerar alertas"""
        alerts = []
        
        if metrics['p95_analysis_time'] > self.thresholds['analysis_time_p95']:
            alerts.append(f"High analysis time: {metrics['p95_analysis_time']:.2f}s")
        
        if metrics['cache_hit_rate'] < self.thresholds['cache_hit_rate_min']:
            alerts.append(f"Low cache hit rate: {metrics['cache_hit_rate']:.2f}")
        
        if metrics['avg_memory_usage'] > self.thresholds['memory_usage_max']:
            alerts.append(f"High memory usage: {metrics['avg_memory_usage']:.0f}MB")
        
        return alerts
```

## 🔧 **Configuration**

### Configuração Padrão
```json
{
  "context_engine": {
    "max_files": 1000,
    "max_context_size": 50000,
    "max_context_files": 8,
    "cache_ttl_hours": 24,
    "cache_max_size_mb": 500,
    "supported_languages": ["python", "javascript", "typescript", "go", "rust"],
    "ignore_patterns": [".git", "__pycache__", "node_modules", ".env"],
    "performance_mode": "balanced"
  }
}
```

### Modos de Performance
```python
PERFORMANCE_MODES = {
    'fast': {
        'max_files': 500,
        'max_context_files': 5,
        'enable_ast_parsing': False,
        'enable_graph_analysis': False
    },
    'balanced': {
        'max_files': 1000,
        'max_context_files': 8,
        'enable_ast_parsing': True,
        'enable_graph_analysis': True
    },
    'thorough': {
        'max_files': 2000,
        'max_context_files': 12,
        'enable_ast_parsing': True,
        'enable_graph_analysis': True
    }
}
```

---

**Esta especificação fornece implementação clara e testável do Context Engine, com performance targets realísticos e arquitetura bem definida.**