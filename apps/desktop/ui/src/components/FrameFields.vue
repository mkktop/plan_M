<script setup lang="ts">
import { NInput, NInputNumber } from 'naive-ui'
import type { DraftFrame } from '@/utils/packageStudio'

const frame = defineModel<DraftFrame>({ required: true })
</script>

<template>
  <div class="form-grid">
    <label class="field span-2"><span>请求字节</span>
      <n-input v-model:value="frame.request" size="small" placeholder="例如 {unit} 10" class="mono" />
    </label>
    <label class="field"><span>应答前缀</span>
      <n-input v-model:value="frame.responsePrefix" size="small" placeholder="例如 {unit} 90" class="mono" />
    </label>
    <label class="field"><span>应答总字节数</span>
      <n-input-number v-model:value="frame.responseBytes" size="small" :min="1" :max="512" :precision="0" />
    </label>
  </div>
</template>
