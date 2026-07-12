import { onMounted, onUnmounted, type Ref, watch } from 'vue'

interface Particle {
  x: number
  y: number
  baseY: number
  size: number
  speedX: number
  amplitude: number
  frequency: number
  phase: number
  opacity: number
}

export function useParticles(
  canvasRef: Ref<HTMLCanvasElement | null>,
  active: Ref<boolean>,
) {
  let ctx: CanvasRenderingContext2D | null = null
  let particles: Particle[] = []
  let animationId: number | null = null

  function maxParticleCount() {
    const canvas = canvasRef.value
    if (!canvas) return 40
    const isMobile = window.innerWidth < 768
    const cap = isMobile ? 40 : 80
    return Math.min(cap, Math.floor((canvas.width * canvas.height) / 15000))
  }

  function resize() {
    const canvas = canvasRef.value
    if (!canvas) return
    canvas.width = window.innerWidth
    canvas.height = window.innerHeight
  }

  function createParticles() {
    const canvas = canvasRef.value
    if (!canvas) return
    const count = maxParticleCount()
    particles = Array.from({ length: count }, () => {
      const y = Math.random() * canvas.height
      return {
        x: Math.random() * canvas.width,
        y,
        baseY: y,
        size: Math.random() * 3 + 1.5,
        speedX: Math.random() * 0.4 + 0.15,
        amplitude: Math.random() * 25 + 10,
        frequency: Math.random() * 0.008 + 0.003,
        phase: Math.random() * Math.PI * 2,
        opacity: Math.random() * 0.35 + 0.15,
      }
    })
  }

  function draw() {
    const canvas = canvasRef.value
    if (!ctx || !canvas) return

    ctx.clearRect(0, 0, canvas.width, canvas.height)
    const time = Date.now() * 0.001
    const w = canvas.width
    const h = canvas.height

    for (const p of particles) {
      p.x += p.speedX
      p.y = p.baseY + Math.sin(time * 0.8 + p.phase + p.x * p.frequency) * p.amplitude

      if (p.x > w + 20) {
        p.x = -20
        p.baseY = Math.random() * h
      }

      const glowRadius = Math.max(0.1, p.size * 4)
      const gradient = ctx.createRadialGradient(p.x, p.y, 0, p.x, p.y, glowRadius)
      gradient.addColorStop(0, `rgba(255,255,255,${p.opacity * 0.6})`)
      gradient.addColorStop(0.3, `rgba(200,220,255,${p.opacity * 0.25})`)
      gradient.addColorStop(1, 'rgba(255,255,255,0)')

      ctx.fillStyle = gradient
      ctx.beginPath()
      ctx.arc(p.x, p.y, glowRadius, 0, Math.PI * 2)
      ctx.fill()

      ctx.beginPath()
      ctx.arc(p.x, p.y, p.size, 0, Math.PI * 2)
      ctx.fillStyle = `rgba(255,255,255,${p.opacity})`
      ctx.fill()
    }

    animationId = requestAnimationFrame(draw)
  }

  function start() {
    const canvas = canvasRef.value
    if (!canvas) return
    ctx = canvas.getContext('2d')
    resize()
    createParticles()
    if (animationId) cancelAnimationFrame(animationId)
    draw()
  }

  function stop() {
    if (animationId) {
      cancelAnimationFrame(animationId)
      animationId = null
    }
    if (ctx && canvasRef.value) {
      ctx.clearRect(0, 0, canvasRef.value.width, canvasRef.value.height)
    }
  }

  function onResize() {
    resize()
    createParticles()
  }

  watch(active, (enabled) => {
    if (enabled) start()
    else stop()
  })

  onMounted(() => {
    window.addEventListener('resize', onResize)
    if (active.value) start()
  })

  onUnmounted(() => {
    window.removeEventListener('resize', onResize)
    stop()
  })

  return { start, stop }
}
