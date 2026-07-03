# ThreadShape v5 実装仕様（UI再構築: ドッキング・レイヤー・デザイントークン）

対象: `threadtype-egui/`。現状 `cargo test` 28件グリーン・警告0。壊さないこと。
`src/geometry.rs` は変更禁止。キャンバスの描画・入力ロジック（canvas 関数の中身）は
「どのパネルに置くか」を変えるだけで、挙動を変えないこと。

## 参照デザイン

既存のデザインシステムのトークンに合わせる。
Apple 系のニュートラルなツール UI。値は以下をそのまま使う:

- ライト: canvas #e8e8e8 / panel #ffffff / bg-1 #f7f7f7 / bg-2 #efefef /
  border #00000012（強 #00000020）/ text-1 #1d1d1f / text-2 #6e6e73 / text-3 #aeaeb2 /
  accent #007aff（bg #007aff14）/ warning #ff9f0a / error #ff3b30
- ダーク: canvas #141517 / panel #1e1e1e / bg-1 #252525 / bg-2 #2d2d2d /
  border #ffffff14（強 #ffffff22）/ text-1 #f5f5f7 / text-2 #a1a1a6 / text-3 #636366 /
  accent #0a84ff（bg #0a84ff20）/ warning #ffa00a / error #ff453a
- タイポ: UI 12px / ラベル 11px（letter-spacing 0.05em 相当、egui では近似でよい）
- 角丸: コントロール 6px / パネル 10px。行高: コンパクト 24px / 標準 28px。
  ツールバー高 32px / ステータスバー高 24px。
- キャンバス内の糸・面・ペグの配色は現行のモノクロ方針を維持しつつ、
  選択色 = accent、警告 = error、確定/成功 = 緑系のまま。

egui で再現できない要素（squircle、影の厳密な再現）は角丸+細ボーダーで近似する。

## WP1 テーマ/スタイル基盤

- 既存 `Theme` 構造体を上記トークンで作り直す（dark()/light()）。
- `egui::Style` も連動設定: text_styles（Body/Button=12px, Small=11px,
  Heading=13px 500相当）、spacing（item_spacing, button_padding,
  interact_size.y = 22.0）、visuals（widgets の bg/border を bg-1/bg-2/border で、
  rounding 6.0、window_rounding 10.0、selection.bg_fill = accent-bg）。
- テーマ切替は現行トグルを維持。

## WP2 アプリの骨格（ツールバー / ステータスバー）

- 上部 `TopBottomPanel::top`（高さ32px）: 左からツールボタン群
  （選択 V / ダイレクト選択 A / 描画 P / 任意円 C — 記号かアイコン風の短いラベルで、
  アクティブは accent-bg + accent 文字）、区切り、元に戻す/やり直す、区切り、
  SVG書き出し / JSON保存 / JSON読み込み、右端にテーマ切替と「円を表示(H)」。
- 下部 `TopBottomPanel::bottom`（24px）: ステータスメッセージ（現行 status）
  + 右端にズーム%表示。
- 既存サイドパネル内の同機能は撤去（重複させない）。

## WP3 ドッキング / フローティング（egui_dock）

- 依存追加: `egui_dock = "0.14"`（egui 0.29 対応版）。
- タブ enum: `Canvas / Layers / Circles / Properties / Grid`。
- 初期レイアウト: 中央 Canvas、右に幅 240px の縦分割で
  上から Layers、Circles、Properties。Grid は Properties とタブ同居。
- Canvas タブは閉じ不可・移動不可（`TabViewer::is_closeable` 等で制御）。
  他タブはドラッグで分割・タブ同居・**フローティングウィンドウ化**
  （egui_dock の undock/window 機能を有効化）ができる。
- 実装パターン: `App` に `dock: DockState<Tab>` を保持し、update() では
  `let mut dock = std::mem::replace(&mut self.dock, DockState::new(vec![]));`
  で取り出してから `DockArea::new(&mut dock).show(ctx, &mut Viewer { app: self })`、
  最後に書き戻す（App と DockState の二重借用を避ける）。
- 既存の canvas(ui) / 各パネル UI をタブの中身として移植する。挙動は不変。

## WP4 ネーミング整理（一覧 = レイヤー）

- 「シェイプ一覧」→「**レイヤー**」パネル。`Face` に
  `#[serde(default)] name: String` を追加（空なら「レイヤー N」を表示）。
  行 = 参考モックの Rows/List Items: [目アイコン(表示トグル)] [名前] を
  行高24pxで、選択行は accent-bg。行クリックで選択、ダブルクリックで名前編集
  （`TextEdit`）。削除は行の右端に小さな「×」。並び順は上が手前（描画順の逆）。
- 「任意円一覧」→「**円**」パネル。行 = [目] [円 #id] [x/y/r の DragValue]。
- 「**プロパティ**」パネル: 選択対象の詳細。レイヤー選択時 = 名前・表示。
  円選択時 = X / Y / 半径（`DragValue`、mono 表示）。未選択時 = 「選択なし」。
- 「**グリッド**」パネル: 間隔・円の半径・引っかかり範囲・サブ円・
  間隔連動・拡縮基準（現行機能の移設）。
- UI 文言も「面」→「レイヤー」に統一（ステータスメッセージ含む）。

## WP5 Shift+クリック複数選択

- `selected: Option<Selection>` → `selection: Vec<Selection>` に変更
  （順序保持・重複なし。ヘルパー `is_selected / toggle / set_single / clear`）。
- キャンバス・レイヤーパネル・円パネルで Shift+クリック = トグル追加、
  通常クリック = 単独選択。
- Delete = 選択全部を削除（参照中の円はスキップし、その旨ステータス表示）。
  履歴は一括で1回。
- 表示トグルをプロパティパネルから一括適用できる（選択に複数レイヤーが
  ある場合は「表示」「非表示」ボタンで一括設定）。
- 選択ツールでのドラッグ移動: 複数の任意円が選択されているときは
  まとめて平行移動（履歴1回、変更前スナップショット方式を厳守）。

## WP6 仕上げ

- README 更新（UI 構成・タブの説明）。
- `cargo test` 全件グリーン・`cargo build` 警告0。
- 新規テスト: 選択モデルのトグル/単独選択/一括削除のユニットテスト
  （UI 非依存のロジック部分を関数に切り出してテストする）。

## 注意

- 履歴は従来どおり「変更前スナップショット」パターン（push_history_snapshot）。
- キャンバスのショートカット（V/A/P/C/H/Esc/Delete/Cmd+Z系）は維持。
  ただしテキスト編集中（名前編集など）はショートカットを発火させない
  （`ctx.wants_keyboard_input()` でガード）。
- egui_dock のバージョンが egui 0.29 と合わない場合は合う最新版を選び、
  API 差分は egui_dock 側のドキュメントに従う（egui 本体のバージョンは変えない）。
- フローティング機能が使用バージョンで未提供の場合は、タブのドラッグ分割までを
  必須とし、フローティングは `egui::Window` による「切り離し」ボタンで代替する。
