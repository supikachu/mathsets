<template>
  <nav class="bottom-nav">
    <router-link
      v-for="item in items"
      :key="item.path"
      :to="item.path"
      class="nav-item"
      :class="{ active: isActive(item.path) }"
    >
      <AppIcon :name="item.icon" :size="24" />
      <span>{{ item.shortLabel }}</span>
    </router-link>
  </nav>
</template>

<script setup lang="ts">
import { useRoute } from 'vue-router'
import { useNavItems } from '@/composables/useNavItems'
import { AppIcon } from '@/components/ui'

const route = useRoute()
const { items } = useNavItems()

function isActive(path: string) {
  return route.path === path || route.path.startsWith(path + '/')
}
</script>
