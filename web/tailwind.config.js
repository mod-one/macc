/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        bg: {
          primary: 'var(--bg-primary)',
          secondary: 'var(--bg-secondary)',
          card: 'var(--bg-card)',
        },
        text: {
          primary: 'var(--text-primary)',
          secondary: 'var(--text-secondary)',
          muted: 'var(--text-muted)',
        },
        border: 'var(--border)',
        accent: 'var(--accent)',
        success: 'var(--success)',
        warning: 'var(--warning)',
        error: 'var(--error)',
        info: 'var(--info)',
        status: {
          todo: 'var(--status-todo)',
          active: 'var(--status-active)',
          blocked: 'var(--status-blocked)',
          merged: 'var(--status-merged)',
          failed: 'var(--status-failed)',
          paused: 'var(--status-paused)',
        }
      },
      fontFamily: {
        ui: 'var(--font-ui)',
        mono: 'var(--font-mono)',
      },
      spacing: {
        base: 'var(--padding-base)',
        gap: 'var(--gap-base)',
      },
      borderRadius: {
        card: 'var(--radius-card)',
      },
      boxShadow: {
        soft: 'var(--shadow-soft)',
      }
    },
  },
  plugins: [],
}
