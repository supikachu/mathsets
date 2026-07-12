import { ref, watch } from 'vue'

const STORAGE_KEY = 'mathset_particle_bg'

const particlesEnabled = ref(localStorage.getItem(STORAGE_KEY) === 'true')

export function useParticlePreference() {
  function setParticlesEnabled(value: boolean) {
    particlesEnabled.value = value
    localStorage.setItem(STORAGE_KEY, String(value))
  }

  return {
    particlesEnabled,
    setParticlesEnabled,
  }
}
