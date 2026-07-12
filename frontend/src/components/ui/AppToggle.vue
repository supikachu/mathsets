<script setup lang="ts">
const model = defineModel<boolean>({ required: true })

function toggle() {
  model.value = !model.value
}
</script>

<template>
  <button
    type="button"
    class="toggle"
    :class="{ active: model }"
    role="switch"
    :aria-checked="model"
    @click="toggle"
  />
</template>
