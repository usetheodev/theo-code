/** @type {import('tailwindcss').Config} */
export default {
    darkMode: ["class"],
    content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
  	extend: {
  		colors: {
  			surface: {
  				'0': '#08090c',
  				'1': '#0e1117',
  				'2': '#151921',
  				'3': '#1c2130',
  				'4': '#252b3b'
  			},
  			border: {
  				DEFAULT: '#1e2433',
  				strong: '#2b3348',
  				focus: '#5b4dc7'
  			},
  			text: {
  				'0': '#f0f2f5',
  				'1': '#c0c8d8',
  				'2': '#7c879e',
  				'3': '#4e586e'
  			},
  			brand: {
  				DEFAULT: '#6c5ce7',
  				hover: '#5a49d6',
  				soft: 'rgba(108, 92, 231, 0.08)',
  				glow: 'rgba(108, 92, 231, 0.19)'
  			},
  			ok: '#10b981',
  			warn: '#f59e0b',
  			err: '#ef4444',
  			info: '#3b82f6',
  			sidebar: {
  				DEFAULT: 'hsl(var(--sidebar-background))',
  				foreground: 'hsl(var(--sidebar-foreground))',
  				primary: 'hsl(var(--sidebar-primary))',
  				'primary-foreground': 'hsl(var(--sidebar-primary-foreground))',
  				accent: 'hsl(var(--sidebar-accent))',
  				'accent-foreground': 'hsl(var(--sidebar-accent-foreground))',
  				border: 'hsl(var(--sidebar-border))',
  				ring: 'hsl(var(--sidebar-ring))'
  			}
  		},
  		fontFamily: {
  			sans: [
  				'Inter',
  				'ui-sans-serif',
  				'system-ui',
  				'-apple-system',
  				'sans-serif'
  			],
  			mono: [
  				'JetBrains Mono',
  				'ui-monospace',
  				'Fira Code',
  				'monospace'
  			]
  		},
  		animation: {
  			'fade-in': 'fade-in 0.3s ease-out both',
  			'pulse-dot': 'pulse-dot 1.5s ease-in-out infinite'
  		},
  		keyframes: {
  			'fade-in': {
  				from: {
  					opacity: '0',
  					transform: 'translateY(6px)'
  				},
  				to: {
  					opacity: '1',
  					transform: 'translateY(0)'
  				}
  			},
  			'pulse-dot': {
  				'0%, 100%': {
  					opacity: '0.4'
  				},
  				'50%': {
  					opacity: '1'
  				}
  			}
  		}
  	}
  },
  plugins: [],
};
