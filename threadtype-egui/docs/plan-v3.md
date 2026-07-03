# ThreadShape v3 実装仕様（ペグモデル再設計）

対象: `threadtype-egui/`。現状 `cargo test` 19件グリーン・警告0。この状態を壊さないこと。
`src/geometry.rs` のソルバー（update_thread / 接線関数 / thread_to_wrap_elements /
materialize_chain）は摩擦モデル対応済みで正しく動いている。**リファクタリング禁止・変更禁止**。
今回の作業はモデル（model.rs）とアプリ統合（app.rs / export.rs）が中心。

## 概念の整理（ユーザーフィードバック反映）

円は3種類になる。従来の「サブ円ツール（自由配置）」は廃止し、以下に置き換える:

1. **メイン円** — グリッド格子点。従来どおり `PegRef::Grid { row, col }`
2. **サブ円** — メイン円の「間」（セルの中心）に自動配置される小さい円。
   自由配置ではない。密度（間隔）はメインのグリッドに追従し、半径だけを
   独立に調整できる。`PegRef::Sub { row, col }`（セル (row,col)〜(row+1,col+1) の中心）
3. **任意円** — 一つ一つが自分の位置と半径を持つ円。配置・移動・個別リサイズ・
   削除ができる。現行の `CustomPeg` / `PegRef::Custom { id }` を流用

3種とも巻き付き対象・開始十字点の対象として完全に同格（ペグ解決で `Vec<Peg>` に
入れば geometry 側は何もしなくてよい）。

## データモデル（model.rs、JSON version = 3。旧版との互換不要）

```rust
pub enum PegRef {
    Grid { row: i32, col: i32 },
    Sub { row: i32, col: i32 },   // 追加
    Custom { id: u64 },
}

pub enum ScaleAnchor { TopLeft, Center }

pub struct GridSettings {
    pub spacing: f64,
    pub radius: f64,
    pub catch: f64,
    pub sub_radius: f64,
    pub subs_enabled: bool,      // サブ円を使うか。デフォルト true
    pub scale_link: bool,        // 間隔変更時に半径も比例スケールするか。デフォルト false
    pub scale_anchor: ScaleAnchor, // 間隔変更の基準。デフォルト TopLeft
}
```

## ペグ解決の一元化（重要）

現在ペグ解決ロジックが3箇所に重複している（app::resolve_peg / canvas 内クロージャ /
export::resolve_peg）。これを **model.rs の1関数に統合**する:

```rust
pub fn resolve_peg(
    grid: &GridSettings,
    customs: &[CustomPeg],
    canvas_center: Point,   // 中央基準スケーリングに必要
    peg: &PegRef,
) -> Option<Peg>
```

- 左上基準（TopLeft）: origin = (110, 110) **固定**（現在の `(spacing*0.55).max(90)`
  は間隔変更で原点まで動いてしまうため廃止）。
  - Grid: origin + (col·spacing, row·spacing)、半径 = radius + catch
  - Sub: origin + ((col+0.5)·spacing, (row+0.5)·spacing)、半径 = sub_radius + catch
- 中央基準（Center）: 基準セル (row=2, col=3) がキャンバス中心に固定される:
  位置 = canvas_center + ((col−3)·spacing, (row−2)·spacing)（Sub は +0.5 同様）。
  間隔を変えると中心の円は動かず周囲が広がる/縮む。
- Custom: customs から id 検索、半径 = peg.radius + catch。

app の `engine_pegs`（可視ペグ列挙）もこの関数ベースに書き直す。可視範囲の
row/col は origin とキャンバスサイズから逆算する（負の row/col も中央基準では
あり得る点に注意）。export（SVG）は App が保持する最新キャンバス中心を
`document_to_svg(doc, canvas_center)` として渡す形に変える。

## 操作仕様

### サブ円

- サイドパネル: 「サブ円を使う」チェックボックス + 「サブ円の半径」スライダー
  （8〜60、デフォルト24）。
- ON のとき全セル中心にサブ円が現れ、巻き付き・開始点・十字マーカーが機能する。
- OFF にしたとき、サブ円を参照する面がある場合は OFF を拒否せず、該当面を
  赤輪郭警告（materialize_face が None になるケースなので、None の面は
  「解決不能」として一覧から消さず赤い破線マーカーか状態メッセージで知らせる。
  最低限: ステータスバーに「サブ円を参照する面があります」と表示すれば可）。

### 任意円

- ツールは3つ: 「描画 / 選択 / 任意円」（サブ円ツールを任意円ツールに改名）。
- 任意円ツール: 空きスペースクリックで配置（半径はパネルの「任意円の半径」
  スライダー値、デフォルト40。重なり配置の拒否は現行踏襲）。
- 選択ツール:
  - 任意円をクリックで選択（現行）。**ドラッグで移動**（参照中の面があっても
    移動可 — 面はパラメトリックなので追従する。これが任意円の狙い）。
    移動確定（ドラッグ終了）時に1回だけ履歴を積む（ドラッグ中は積まない）。
  - 選択中はパネルに「半径」スライダー（8〜120）を表示し、その円の radius を
    直接編集（変更確定はスライダーのドラッグ終了で履歴1回。egui の
    `Response::drag_stopped()` を使う）。
  - Delete で削除（面が参照中なら拒否、現行踏襲）。
- 描画中（self.draw が Some）のペグ移動・削除は不可（ツール切替時に描画キャンセル）。

### 密度（間隔）変更のオプション

- 「間隔に合わせて円の大きさも変える」チェックボックス（scale_link）:
  ON のとき間隔スライダーの変化率で radius と sub_radius も比例スケール
  （スライダー範囲でクランプ）。任意円の半径は変えない。
- 「拡縮の基準: 左上 / 中央」ラジオ（scale_anchor）: 上記解決式の切替。

## 作業パッケージ（順に。各WP後に cargo test）

- **WP1**: model.rs 改訂（PegRef::Sub / ScaleAnchor / GridSettings 拡張 /
  resolve_peg 一元化関数 + 単体テスト: TopLeft と Center の位置式、Sub の位置、
  Center モードで spacing を変えても基準セル (2,3) の位置が不変であること）
- **WP2**: app.rs / export.rs を resolve_peg 一元化に載せ替え（挙動不変の確認）
- **WP3**: サブ円（生成・トグル・半径スライダー）
- **WP4**: 任意円ツール改名と配置、選択ドラッグ移動、個別半径編集、履歴の
  ドラッグ終了時1回ルール
- **WP5**: 密度オプション（scale_link / scale_anchor UI と挙動）
- **WP6**: README 更新、全テストグリーン、警告0

## 注意

- geometry.rs は変更しないこと（`resolve_anchor` など必要な公開関数の追加のみ可）。
- 描画開始の十字点・摩擦モデル・Command Pin・閉じ判定のロジックは触らない。
- 日本語UI・履歴の意味単位ルールを維持。
