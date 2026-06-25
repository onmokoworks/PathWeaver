import type { GlyphContour, GridSettings } from '../state/types'

type Props = {
  canRedo: boolean
  canUndo: boolean
  gridSettings: GridSettings
  selectedContour: GlyphContour | undefined
  onChangeGrid: <K extends keyof GridSettings>(key: K, value: GridSettings[K]) => void
  onDeleteSelected: () => void
  onClearGlyph: () => void
  onUndo: () => void
  onRedo: () => void
  onExportSvg: () => void
}

export function ControlPanel({
  canRedo,
  canUndo,
  gridSettings,
  selectedContour,
  onChangeGrid,
  onDeleteSelected,
  onClearGlyph,
  onUndo,
  onRedo,
  onExportSvg,
}: Props) {
  return (
    <aside className="settings-panel">
      <div className="toolbar">
        <button disabled={!canUndo} type="button" onClick={onUndo}>
          元に戻す
        </button>
        <button disabled={!canRedo} type="button" onClick={onRedo}>
          やり直す
        </button>
      </div>

      <div className="control-group">
        <span>ピン</span>
        <label>
          <span>半径</span>
          <input
            max="120"
            min="10"
            step="1"
            type="range"
            value={gridSettings.radius}
            onChange={(event) => onChangeGrid('radius', Number(event.target.value))}
          />
        </label>
        <label>
          <span>間隔</span>
          <input
            max="260"
            min="72"
            step="4"
            type="range"
            value={gridSettings.spacing}
            onChange={(event) => onChangeGrid('spacing', Number(event.target.value))}
          />
        </label>
        <label>
          <span>原点 X</span>
          <input
            max="800"
            min="200"
            step="5"
            type="range"
            value={gridSettings.originX}
            onChange={(event) => onChangeGrid('originX', Number(event.target.value))}
          />
        </label>
        <label>
          <span>原点 Y</span>
          <input
            max="800"
            min="200"
            step="5"
            type="range"
            value={gridSettings.originY}
            onChange={(event) => onChangeGrid('originY', Number(event.target.value))}
          />
        </label>
        <label>
          <span>表示行数</span>
          <input
            max="15"
            min="1"
            step="1"
            type="range"
            value={gridSettings.visibleRows}
            onChange={(event) => onChangeGrid('visibleRows', Number(event.target.value))}
          />
        </label>
        <label>
          <span>表示列数</span>
          <input
            max="15"
            min="1"
            step="1"
            type="range"
            value={gridSettings.visibleCols}
            onChange={(event) => onChangeGrid('visibleCols', Number(event.target.value))}
          />
        </label>
        <label className="toggle-row">
          <input
            checked={gridSettings.visible}
            type="checkbox"
            onChange={(event) => onChangeGrid('visible', event.target.checked)}
          />
          <span>表示</span>
        </label>
      </div>

      <div className="control-group selected-controls">
        <span>選択中の輪郭</span>
        <button disabled={!selectedContour} type="button" onClick={onDeleteSelected}>
          輪郭を削除
        </button>
        <button className="danger" type="button" onClick={onClearGlyph}>
          すべてクリア
        </button>
      </div>

      <div className="export-group">
        <button type="button" onClick={onExportSvg}>
          SVGを書き出し
        </button>
      </div>
    </aside>
  )
}
