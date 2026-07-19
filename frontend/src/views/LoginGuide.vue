<script setup lang="ts">
import AppIcon from '../components/AppIcon.vue'

defineProps<{
  busy?: boolean
  message?: string | null
}>()

defineEmits<{
  login: []
}>()
</script>

<template>
  <main class="login-guide section-surface">
    <div class="login-guide__mark" aria-hidden="true">S</div>
    <p class="eyebrow">SevnX 监控</p>
    <h1>查看账户与聚合用量</h1>
    <p class="login-guide__copy">您未登录，请点击下方按钮登录</p>
    <button class="primary-button login-guide__button" type="button" :disabled="busy" @click="$emit('login')">
      <AppIcon name="login" :size="17" />
      <span>{{ busy ? '正在打开登录窗口' : '登录 SevnX' }}</span>
    </button>
    <p v-if="message" class="inline-error" role="alert">{{ message }}</p>
    <p class="login-guide__hint">登录将在应用内窗口中完成，凭证不会显示在此页面。</p>
  </main>
</template>
