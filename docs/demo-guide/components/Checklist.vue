<script setup lang="ts">
defineProps<{ items: string[] }>()
</script>
<template>
  <ul class="my-4 space-y-2 text-sm">
    <li v-for="item in items" :key="item" class="flex items-center gap-2">
      <span class="text-green-500 text-lg">✓</span>
      <span>{{ item }}</span>
    </li>
  </ul>
</template>
