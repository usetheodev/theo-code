//! HTML template with inline CSS/JS — theo-desktop design system.

/// Build the complete self-contained HTML page.
pub fn build_html(title: &str, sidebar_html: &str, pages_html: &str, search_index: &str) -> String {
    format!(r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{title}</title>
<link rel="preconnect" href="https://fonts.googleapis.com">
<link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600&family=JetBrains+Mono:wght@400;500&display=swap" rel="stylesheet">
<style>
:root {{
  --surface-0: #08090c;
  --surface-1: #0e1117;
  --surface-2: #151921;
  --surface-3: #1c2130;
  --surface-4: #252b3b;
  --text-0: #f0f2f5;
  --text-1: #c0c8d8;
  --text-2: #7c879e;
  --text-3: #4e586e;
  --brand: #6c5ce7;
  --brand-hover: #5a49d6;
  --brand-soft: rgba(108, 92, 231, 0.08);
  --brand-glow: rgba(108, 92, 231, 0.19);
  --border: #1e2433;
  --border-strong: #2b3348;
  --ok: #10b981;
  --warn: #f59e0b;
  --err: #ef4444;
  --info: #3b82f6;
  --sidebar-w: 280px;
}}

* {{ margin: 0; padding: 0; box-sizing: border-box; }}

body {{
  font-family: 'Inter', -apple-system, BlinkMacSystemFont, sans-serif;
  background: var(--surface-0);
  color: var(--text-0);
  font-size: 14px;
  line-height: 1.6;
  -webkit-font-smoothing: antialiased;
}}

/* Layout */
.layout {{ display: flex; min-height: 100vh; }}

/* Sidebar */
.sidebar {{
  width: var(--sidebar-w);
  background: var(--surface-1);
  border-right: 1px solid var(--border);
  overflow-y: auto;
  position: fixed;
  top: 0;
  left: 0;
  bottom: 0;
  padding: 0;
  z-index: 10;
}}

.sidebar-header {{
  padding: 20px 16px 12px;
  border-bottom: 1px solid var(--border);
  position: sticky;
  top: 0;
  background: var(--surface-1);
  z-index: 1;
}}

.sidebar-title {{
  font-size: 15px;
  font-weight: 600;
  color: var(--text-0);
  display: flex;
  align-items: center;
  gap: 8px;
}}

.sidebar-title svg {{ width: 18px; height: 18px; color: var(--brand); }}

.search-box {{
  margin-top: 12px;
  position: relative;
}}

.search-box input {{
  width: 100%;
  padding: 8px 12px 8px 32px;
  background: var(--surface-2);
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--text-0);
  font-size: 13px;
  font-family: inherit;
  outline: none;
  transition: border-color 0.15s;
}}

.search-box input:focus {{ border-color: var(--brand); }}

.search-box svg {{
  position: absolute;
  left: 10px;
  top: 50%;
  transform: translateY(-50%);
  width: 14px;
  height: 14px;
  color: var(--text-3);
}}

.sidebar-nav {{ padding: 8px 0; }}

.nav-group {{ padding: 4px 0; }}

.nav-group-label {{
  padding: 8px 16px 4px;
  font-size: 11px;
  font-weight: 600;
  color: var(--text-3);
  text-transform: uppercase;
  letter-spacing: 0.05em;
}}

.nav-item {{
  display: block;
  padding: 6px 16px 6px 24px;
  font-size: 13px;
  color: var(--text-1);
  text-decoration: none;
  cursor: pointer;
  border-radius: 0;
  transition: all 0.1s;
  border-left: 2px solid transparent;
}}

.nav-item:hover {{
  background: var(--brand-soft);
  color: var(--text-0);
}}

.nav-item.active {{
  color: var(--brand);
  background: var(--brand-soft);
  border-left-color: var(--brand);
  font-weight: 500;
}}

/* Content */
.content {{
  margin-left: var(--sidebar-w);
  flex: 1;
  max-width: 100%;
  padding: 40px 48px;
}}

.page {{ animation: fadeIn 0.2s ease; }}

@keyframes fadeIn {{ from {{ opacity: 0; transform: translateY(4px); }} to {{ opacity: 1; transform: translateY(0); }} }}

/* Typography */
.page h1 {{ font-size: 28px; font-weight: 600; color: var(--text-0); margin: 0 0 16px; padding-bottom: 12px; border-bottom: 1px solid var(--border); }}
.page h2 {{ font-size: 20px; font-weight: 600; color: var(--text-0); margin: 32px 0 12px; }}
.page h3 {{ font-size: 16px; font-weight: 600; color: var(--text-1); margin: 24px 0 8px; }}
.page p {{ color: var(--text-1); margin: 8px 0; }}
.page strong {{ color: var(--text-0); }}
.page a {{ color: var(--brand); text-decoration: none; }}
.page a:hover {{ text-decoration: underline; }}

.wiki-link {{
  color: var(--brand);
  cursor: pointer;
  border-bottom: 1px dashed var(--brand);
  padding-bottom: 1px;
}}
.wiki-link:hover {{ color: var(--brand-hover); }}

/* Lists */
.page ul, .page ol {{ padding-left: 20px; margin: 8px 0; }}
.page li {{ color: var(--text-1); margin: 4px 0; }}

/* Tables */
.page table {{ width: 100%; border-collapse: collapse; margin: 16px 0; font-size: 13px; }}
.page th {{ text-align: left; padding: 8px 12px; background: var(--surface-2); color: var(--text-0); border: 1px solid var(--border); font-weight: 500; }}
.page td {{ padding: 8px 12px; border: 1px solid var(--border); color: var(--text-1); }}
.page tr:hover td {{ background: var(--surface-1); }}

/* Code */
.page code {{
  font-family: 'JetBrains Mono', 'Fira Code', monospace;
  font-size: 12.5px;
  background: var(--surface-2);
  padding: 2px 6px;
  border-radius: 4px;
  color: var(--brand);
}}

.page pre {{
  background: var(--surface-1);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 16px;
  overflow-x: auto;
  margin: 16px 0;
}}

.page pre code {{
  background: none;
  padding: 0;
  color: var(--text-0);
  font-size: 13px;
  line-height: 1.5;
}}

/* Blockquotes */
.page blockquote {{
  border-left: 3px solid var(--brand);
  padding: 8px 16px;
  margin: 12px 0;
  background: var(--brand-soft);
  border-radius: 0 6px 6px 0;
  color: var(--text-1);
  font-size: 13px;
}}

/* HR */
.page hr {{ border: none; border-top: 1px solid var(--border); margin: 24px 0; }}

/* Scrollbar */
::-webkit-scrollbar {{ width: 6px; }}
::-webkit-scrollbar-track {{ background: transparent; }}
::-webkit-scrollbar-thumb {{ background: var(--surface-4); border-radius: 3px; }}
::-webkit-scrollbar-thumb:hover {{ background: var(--text-3); }}

/* Search results */
.search-results {{ display: none; padding: 8px 0; }}
.search-results.visible {{ display: block; }}
.search-result {{
  display: block;
  padding: 6px 16px 6px 24px;
  font-size: 13px;
  color: var(--info);
  cursor: pointer;
}}
.search-result:hover {{ background: var(--brand-soft); }}

/* Responsive */
@media (max-width: 768px) {{
  .sidebar {{ width: 100%; position: relative; border-right: none; border-bottom: 1px solid var(--border); }}
  .content {{ margin-left: 0; padding: 24px 16px; }}
  .layout {{ flex-direction: column; }}
}}

/* Badge */
.badge {{
  display: inline-block;
  padding: 2px 8px;
  border-radius: 4px;
  font-size: 11px;
  font-weight: 500;
}}
.badge-ok {{ background: rgba(16,185,129,0.1); color: var(--ok); }}
.badge-warn {{ background: rgba(245,158,11,0.1); color: var(--warn); }}
</style>
</head>
<body>
<div class="layout">
  <nav class="sidebar">
    <div class="sidebar-header">
      <div class="sidebar-title">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M4 19.5v-15A2.5 2.5 0 0 1 6.5 2H20v20H6.5a2.5 2.5 0 0 1 0-5H20"/></svg>
        {title}
      </div>
      <div class="search-box">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/></svg>
        <input type="text" id="searchInput" placeholder="Search pages..." oninput="onSearch(this.value)">
      </div>
    </div>
    <div class="sidebar-nav" id="sidebarNav">
      {sidebar_html}
    </div>
    <div class="search-results" id="searchResults"></div>
  </nav>
  <main class="content" id="mainContent">
    {pages_html}
  </main>
</div>
<script>
const searchIndex = {search_index};

function showPage(slug) {{
  document.querySelectorAll('.page').forEach(p => p.style.display = 'none');
  const target = document.getElementById('page-' + slug);
  if (target) {{
    target.style.display = 'block';
    target.style.animation = 'none';
    target.offsetHeight;
    target.style.animation = null;
  }}
  document.querySelectorAll('.nav-item').forEach(a => a.classList.remove('active'));
  const link = document.querySelector('.nav-item[data-slug="' + slug + '"]');
  if (link) link.classList.add('active');
  document.getElementById('searchInput').value = '';
  document.getElementById('searchResults').classList.remove('visible');
  document.getElementById('sidebarNav').style.display = '';
  window.scrollTo(0, 0);
}}

function onSearch(query) {{
  const results = document.getElementById('searchResults');
  const nav = document.getElementById('sidebarNav');
  if (!query || query.length < 2) {{
    results.classList.remove('visible');
    nav.style.display = '';
    return;
  }}
  const q = query.toLowerCase();
  const matches = searchIndex.filter(e =>
    e.title.toLowerCase().includes(q) || e.text.toLowerCase().includes(q)
  ).slice(0, 15);
  results.innerHTML = matches.map(m =>
    '<a class="search-result" onclick="showPage(\'' + m.slug + '\')">' + m.title + '</a>'
  ).join('');
  results.classList.add('visible');
  nav.style.display = 'none';
}}
</script>
</body>
</html>"##, title=title, sidebar_html=sidebar_html, pages_html=pages_html, search_index=search_index)
}
