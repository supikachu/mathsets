<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { AppToggle, AppIcon } from '@/components/ui'
import { useParticlePreference } from '@/composables/useParticlePreference'
import { useParticles } from '@/composables/useParticles'

defineProps<{
  title: string
  subtitle?: string
}>()

const canvasRef = ref<HTMLCanvasElement | null>(null)
const { particlesEnabled, setParticlesEnabled } = useParticlePreference()

const reduceMotion = ref(false)

const particlesActive = computed(
  () => particlesEnabled.value && !reduceMotion.value,
)

useParticles(canvasRef, particlesActive)

onMounted(() => {
  reduceMotion.value = window.matchMedia('(prefers-reduced-motion: reduce)').matches
})

function onToggle(value: boolean) {
  setParticlesEnabled(value)
}
</script>

<template>
  <div class="welcome-screen">
    <div class="welcome-static-bg" aria-hidden="true" />
    <canvas
      v-show="particlesActive"
      ref="canvasRef"
      class="particle-canvas"
      aria-hidden="true"
    />

    <div class="welcome-content">
      <AppIcon name="logo" :size="44" class="welcome-logo" />
      <h1 class="welcome-title">{{ title }}</h1>
      <p v-if="subtitle" class="welcome-subtitle">{{ subtitle }}</p>
      <div class="welcome-card">
        <slot />
      </div>
    </div>

    <div v-if="!reduceMotion" class="particle-toggle">
      <span class="particle-toggle-label"><AppIcon name="sparkles" :size="14" />粒子背景</span>
      <AppToggle
        :model-value="particlesEnabled"
        @update:model-value="onToggle"
      />
    </div>
  </div>
</template>

<style scoped>
.welcome-screen {
  position: fixed;
  inset: 0;
  background: #0a0a0f;
  overflow: hidden;
  z-index: 1000;
}

.welcome-static-bg {
  position: absolute;
  inset: 0;
  background:
    radial-gradient(ellipse at 25% 15%, rgba(0, 113, 227, 0.18), transparent 55%),
    radial-gradient(ellipse at 75% 85%, rgba(96, 165, 250, 0.12), transparent 55%),
    radial-gradient(ellipse at 50% 50%, rgba(255, 255, 255, 0.03), transparent 70%);
  pointer-events: none;
}

.particle-canvas {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  pointer-events: none;
}

.welcome-content {
  position: relative;
  z-index: 10;
  min-height: 100vh;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 24px;
  text-align: center;
}

.welcome-title {
  font-size: clamp(28px, 5vw, 42px);
  margin-bottom: 12px;
  color: #ffffff;
  letter-spacing: -0.02em;
  text-shadow: 0 0 40px rgba(255, 255, 255, 0.15);
}

.welcome-subtitle {
  color: rgba(255, 255, 255, 0.5);
  margin-bottom: 28px;
  font-size: 16px;
}

.welcome-card {
  width: 100%;
  max-width: 420px;
  text-align: left;
  padding: 24px;
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid rgba(255, 255, 255, 0.1);
  border-radius: var(--radius-lg);
  box-shadow: 0 4px 24px rgba(0, 0, 0, 0.4);
  color: #ffffff;
  backdrop-filter: blur(20px);
  -webkit-backdrop-filter: blur(20px);
}

.welcome-card :deep(.form-label) {
  color: rgba(255, 255, 255, 0.65);
}

.welcome-card :deep(input) {
  background: rgba(255, 255, 255, 0.08);
  border-color: rgba(255, 255, 255, 0.15);
  color: #ffffff;
}

.welcome-card :deep(input::placeholder) {
  color: rgba(255, 255, 255, 0.35);
}

.welcome-card :deep(input:focus) {
  border-color: rgba(96, 165, 250, 0.8);
  box-shadow: 0 0 0 3px rgba(96, 165, 250, 0.2);
}

.welcome-card :deep(.form-error) {
  color: #ff6b6b;
}

.welcome-card :deep(.auth-footer) {
  text-align: center;
  font-size: 13px;
  color: rgba(255, 255, 255, 0.55);
  margin-top: 8px;
}

.welcome-card :deep(.auth-footer a) {
  color: #5ac8fa;
}

.welcome-card :deep(.auth-title) {
  font-size: 18px;
  font-weight: 700;
  text-align: center;
  margin-bottom: 20px;
  color: #ffffff;
}

.particle-toggle {
  position: fixed;
  bottom: 20px;
  right: 20px;
  z-index: 20;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 14px;
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid rgba(255, 255, 255, 0.1);
  border-radius: var(--radius-md);
  backdrop-filter: blur(12px);
  -webkit-backdrop-filter: blur(12px);
}

.particle-toggle-label {
  font-size: 13px;
  color: rgba(255, 255, 255, 0.7);
  user-select: none;
}

.particle-toggle-label :deep(.app-icon) {
  margin-right: 5px;
}

.welcome-logo {
  color: #ffffff;
  margin-bottom: 16px;
  filter: drop-shadow(0 0 24px rgba(96, 165, 250, 0.4));
}
</style>
