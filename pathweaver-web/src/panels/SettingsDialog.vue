<script setup lang="ts">
// 設定（⌘,）: per-viewer preferences in one place. Changes apply immediately.
import {onBeforeUnmount, onMounted} from 'vue'
import ParameterGrid from '../../vendor/tweeq/src/ParameterGrid/ParameterGrid.vue'
import Parameter from '../../vendor/tweeq/src/ParameterGrid/Parameter.vue'
import ParameterHeading from '../../vendor/tweeq/src/ParameterGrid/ParameterHeading.vue'
import InputNumber from '../../vendor/tweeq/src/InputNumber/InputNumber.vue'
import InputRadio from '../../vendor/tweeq/src/InputRadio/InputRadio.vue'
import InputCheckbox from '../../vendor/tweeq/src/InputCheckbox/InputCheckbox.vue'
import InputButton from '../../vendor/tweeq/src/InputButton/InputButton.vue'
import {resetSettings, settings, type Paper, type Theme, type Wheel} from '../core/settings'

const emit = defineEmits<{close: []}>()
function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape' && !document.querySelector('.Popover:popover-open')) { e.stopPropagation(); emit('close') }
}
onMounted(() => window.addEventListener('keydown', onKey, true))
onBeforeUnmount(() => window.removeEventListener('keydown', onKey, true))
</script>

<template>
  <div class="backdrop" @pointerdown.self="emit('close')">
    <div class="dialog" role="dialog" aria-label="設定" data-testid="settings">
      <header>
        <span class="title">設定</span>
        <InputButton subtle narrow icon="material-symbols:close" aria-label="閉じる" @click="emit('close')" />
      </header>
      <div class="body">
        <ParameterGrid>
          <ParameterHeading>ズームとスクロール</ParameterHeading>
          <Parameter label="ズームの速さ" hint="ピンチと⌘＋ホイールの拡大縮小の速さ（キャンバスとノード共通）">
            <InputNumber data-field="zoom-speed" :model-value="settings.zoomSpeed" :min="0.25" :max="4" :precision="2" :bar="false" suffix="×" @update:model-value="v => { settings.zoomSpeed = v }" />
          </Parameter>
          <Parameter label="ズームの向き">
            <InputCheckbox :model-value="settings.invertZoom" label="反転する" @update:model-value="v => { settings.invertZoom = v }" />
          </Parameter>
          <Parameter label="ホイール" hint="修飾キーなしのスクロール。トラックパッドは「移動」、マウスは「ズーム」が使いやすい">
            <InputRadio data-field="wheel" :model-value="settings.wheel" :options="['pan', 'zoom']" :labels="['移動', 'ズーム']" @update:model-value="v => { settings.wheel = v as Wheel }" />
          </Parameter>

          <ParameterHeading>自動保存</ParameterHeading>
          <Parameter label="間隔" hint="保存していない変更があるとき、この間隔で復元用のコピーを残します。アプリが落ちたり強制終了したりしても、次に開いたときに復元できます">
            <InputRadio data-field="autosave" :model-value="settings.autosave" :options="[0, 1, 5, 10]" :labels="['オフ', '1分', '5分', '10分']" @update:model-value="v => { settings.autosave = v as number }" />
          </Parameter>
          <Parameter label="ファイル" hint="オン：保存したことのあるファイルは、自動保存のたびに上書きします（Pages などと同じ）。オフ：ファイルは ⌘S するまで変わりません">
            <InputCheckbox data-field="autosave-file" :model-value="settings.autosaveFile" :disabled="!settings.autosave" label="保存済みのファイルにも上書き保存する" @update:model-value="v => { settings.autosaveFile = v }" />
          </Parameter>

          <ParameterHeading>表示</ParameterHeading>
          <Parameter label="テーマ">
            <InputRadio data-field="theme" :model-value="settings.theme" :options="['system', 'light', 'dark']" :labels="['システム', 'ライト', 'ダーク']" @update:model-value="v => { settings.theme = v as Theme }" />
          </Parameter>
          <Parameter label="作品の背景" hint="キャンバスの下地だけ。書き出しには含まれません">
            <InputRadio data-field="paper" :model-value="settings.paper" :options="['white', 'gray', 'black', 'checker']" :labels="['白', 'グレー', '黒', '市松']" @update:model-value="v => { settings.paper = v as Paper }" />
          </Parameter>
          <Parameter label="太さハンドル" hint="ダイレクト選択・ペンで常に表示。オフでも⌘を押している間は表示">
            <InputCheckbox :model-value="settings.showWidth" label="常に表示" @update:model-value="v => { settings.showWidth = v }" />
          </Parameter>
        </ParameterGrid>
      </div>
      <footer>
        <InputButton subtle label="初期設定に戻す" @click="resetSettings" />
      </footer>
    </div>
  </div>
</template>

<style scoped>
.backdrop {position: fixed; inset: 0; z-index: 100; display: grid; place-items: center; background: rgba(0, 0, 0, .18)}
.dialog {
  width: min(420px, calc(100vw - 32px)); max-height: calc(100vh - 64px); display: flex; flex-direction: column;
  background: var(--tq-color-background); border: 1px solid var(--tq-color-border); border-radius: 6px;
  box-shadow: 0 12px 40px rgba(0, 0, 0, .25);
}
header {display: flex; align-items: center; height: 36px; padding: 0 6px 0 14px; border-bottom: 1px solid var(--tq-color-border)}
.title {flex: 1; font-weight: 500}
.body {padding: 12px 14px; overflow-y: auto}
footer {display: flex; justify-content: flex-end; padding: 8px 14px; border-top: 1px solid var(--tq-color-border)}
</style>
