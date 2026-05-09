/** @type {import('tailwindcss').Config} */
export default {
  content: ['./index.html', './src/**/*.{vue,ts,tsx}'],
  theme: {
    extend: {},
  },
  plugins: [require('@tailwindcss/typography')],
  // 确保与 Element Plus 不冲突
  important: false,
  corePlugins: {
    preflight: false,
  },
}
