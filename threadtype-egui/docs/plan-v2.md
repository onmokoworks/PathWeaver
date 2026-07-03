# ThreadShape v2 実装仕様（フィードバック反映）

対象クレート: `threadtype-egui/`（Rust + egui/eframe）。
現状: v1 実装済み・`cargo test` 11件グリーン。この状態を壊さず、以下を実装する。

## 反映するフィードバック

1. 線の開始点は円の十字点（円周上の上・右・下・左の4点）のみ。何もない場所からは開始不可
2. 円の半径・間隔を変更したとき、作成済みの形状が追従する
3. サブの小さい円を置ける
4. Command 押下中は円を無視して直線で作図できる。円を突っ切る線は赤くなり、その状態では面を閉じられない
5. 減算モードは削除する
6. （将来・今回は実装しない）円を掴んで位置やサイズを操作する。設計だけ備える

## 設計方針の核: 面のパラメトリック化

面（Face）を焼き込み済みの `Vec<Seg>` ではなく、**ペグ参照ベースのパラメトリック表現**で
保存し、描画・SVG・当たり判定のたびに現在のペグ配置から接線を解いて実体化する。
これが要望 2 の本体であり、要望 6（円の個別操作）の土台になる。

### データモデル（model.rs を全面改訂、JSON version = 2。v1 との互換は不要）

```rust
/// ペグの参照。グリッド由来はセル座標で、追加した円は id で参照する
pub enum PegRef {
    Grid { row: i32, col: i32 },
    Custom { id: u64 },
}

/// ユーザーが置いた円（サブ円）。半径を個別に持つ（将来の個別リサイズ用）
pub struct CustomPeg { pub id: u64, pub center: Point, pub radius: f64 }

pub enum Port { Top, Right, Bottom, Left }

/// 開始点 = ペグの十字点。座標ではなく参照なのでグリッドに追従する
pub struct StartAnchor { pub peg: PegRef, pub port: Port }

pub enum Element {
    /// ペグへの巻き付き。s は巻き方向(±1)、turns は 2π を超えた周回数(通常 0)
    Wrap { peg: PegRef, s: f64, turns: u32 },
    /// Command で打った固定点。絶対座標（グリッドには追従しない。それでよい）
    Pin { pos: Point },
}

pub struct Face { pub start: StartAnchor, pub elements: Vec<Element> }

pub struct GridSettings { pub spacing: f64, pub radius: f64, pub catch: f64, pub sub_radius: f64 }
// sub_radius デフォルト 24.0。FaceMode は削除する

pub struct Document {
    pub version: u32,           // = 2
    pub grid: GridSettings,
    pub custom_pegs: Vec<CustomPeg>,
    pub next_peg_id: u64,
    pub faces: Vec<Face>,
}
```

### ペグ解決

- グリッドペグは Document に保存しない。`PegRef::Grid{row,col}` の位置は
  `origin + (col·spacing, row·spacing)` の式で常に導出する（origin は現状どおり
  マージン定数）。キャンバス外のセルも式で解決できるため、ウィンドウリサイズで
  面が壊れない。
- 実効半径 = Grid ペグは `grid.radius + grid.catch`、Custom ペグは
  `peg.radius + grid.catch`。
- エンジン（geometry.rs のソルバー）は今までどおり `&[Peg]` と添字で動かす。
  毎フレーム「可視グリッドセル + 全 custom_pegs」を `Vec<Peg>` に解決し、
  添字 ↔ PegRef の対応表を並行して持つ（`Vec<PegRef>` でよい）。
  描画中のスレッドの `Wrap.peg: usize` はこの添字。**描画中はこの解決結果を
  凍結する**（現状の「グリッド変更で描画キャンセル」を維持）。

### パラメトリック実体化（geometry.rs に追加する中核関数）

```rust
pub fn materialize_face(face: &Face, resolve: &impl Fn(&PegRef) -> Option<Peg>) -> Option<Vec<Seg>>
```

- チェーン: 開始点（アンカー座標 = ペグ中心 + 実効半径·port方向）
  → 各 Element → 最後にアンカーへ戻る閉路。
- 接線の解決は既存の解析関数を使う:
  - 点（アンカー/Pin）→ 巻き: `point_tangent_entry`
  - 巻き → 点: `point_tangent_exit`
  - 巻き → 巻き: `circle_circle_tangent`（s が同じなら外接線、逆なら内接線）
- 巻きの離脱角は `s·(exit − entry) ∈ (0, 2π]` になるようアンラップし、
  さらに `+ 2π·turns·s`。
- 縮退（acos の引数が範囲外になる等）は clamp して**ベストエフォートで描く**。
  ユーザーデータを消さない。解決不能（参照先ペグ消滅）のときだけ None。
- 描画確定時の変換: ライブの `Wrap { s, entry, exit }` から
  `turns = floor(s·(exit − entry) / 2π)`（amount が (0,2π) なら 0）。

### 向きの正規化（減算削除に伴う変更）

- FaceMode は削除。全面 NonZero 塗りのまま、閉じた面は必ず正の符号付き面積に
  正規化する（重ね塗りが常に合体になる）。
- パラメトリック表現の反転 = elements を逆順にし、各 Wrap の s を反転
  （Pin と turns はそのまま、start は同一点なので不変）。反転しても同じ曲線に
  なることをテストで保証する。

## 操作仕様

### 開始点の十字制限

- 描画ツールで未描画のとき、カーソルに最も近いペグ（中心から実効半径+40px以内）
  の十字4点（円周上の上右下左、実効半径上）に小さなマーカーを表示。
  スナップ距離 12px 以内の点をハイライトする。
- ハイライト中にクリックでそこから描画開始。それ以外の場所のクリックは何もしない
  （ステータスバーに「開始点は円の十字点から選んでください」）。
- 閉じ判定は現状どおりアンカー座標との距離 ≤ CLOSE_RADIUS。

### Command 無視モード

- 描画中に Command（`Modifiers::COMMAND`）押下中:
  - ソルバー（update_thread）を呼ばない。現在の付け根（最後の巻きの離脱点 or
    最後の Pin or アンカー）からカーソルまで直線プレビュー。
  - その直線がいずれかのペグに食い込む（既存の penetration 判定を関数化して流用）
    なら線を赤（例 `Color32::from_rgb(224, 92, 92)`）で描く。
  - クリックで Pin を確定（赤状態でも打てる）。Pin 確定 = 現在のライブ巻き列を
    凍結し、以後のソルバーはその Pin をアンカーとして再開する。
- Command を離すと通常のタウトストリングに戻る。
- 閉じ判定: 面の直線区間（アンカー隣接・Pin 隣接・閉じ線）のどれかがペグに
  食い込んでいる間は閉じられない。閉じリングを赤で表示し、クリックしても
  確定せずステータスに理由を出す。接線・円弧区間はソルバーが保証するので
  チェック不要。
- 確定後の面も毎フレームの実体化時に同じ検査を行い、（グリッド変更などで）
  食い込みが生じた面は輪郭を赤でオーバーレイ表示する（データは保持）。

### サブ円

- ツールに「サブ円」を追加（描画 / 選択 / サブ円 の3つ）。
- サブ円ツール: 空きスペースをクリックで `CustomPeg { radius: grid.sub_radius }`
  を配置（履歴イベント）。既存ペグと重なる位置（中心間距離 < 両実効半径の和）には
  置けない（ステータスに理由）。
- 選択ツール: カスタムペグをクリックで選択でき、Delete で削除。ただし面が
  そのペグを参照している場合は削除を拒否し「この円は面に使われています」。
- サイドパネルに「サブ円の半径」スライダー（8〜60、デフォルト24）。
  変更は**新規配置分のみ**に効く（既存 CustomPeg の radius は保持。
  将来の個別操作の余地）。
- サブ円は巻き付き・開始点・十字点の対象としてメインの円と完全に同格。

### 減算の削除

- FaceMode / 加算・減算トグルを削除。面は常に加算（正面積に正規化）。
- SVG は現状の nonzero のまま。

## 作業パッケージ（この順で。各WP完了ごとに cargo test を通すこと）

- **WP1 モデル改訂**: model.rs を上記の通りに。app.rs / export.rs のコンパイルを
  通すための機械的追従もここで（挙動は仮でよい）。
- **WP2 実体化コア**: `materialize_face` と反転正規化を geometry.rs に実装。
  単体テスト: (a) ソルバーで作った巻き列 → パラメトリック → 実体化が、ライブの
  materialize 結果と 1e-6 で一致（ラウンドトリップ）。(b) 実体化後に
  grid.radius を変えても破綻せず接線が繋がる（区間端点の連続性を検査）。
  (c) 反転しても面積の絶対値が同じで符号だけ変わる。(d) turns=1 の周回が保存される。
- **WP3 アプリ統合**: ペグ解決（グリッド式 + custom_pegs）、面は毎フレーム
  パラメトリックから実体化して描画/当たり判定/SVG に流す。グリッドスライダーで
  既存面が追従することを目視確認。
- **WP4 開始点の十字制限**: マーカー表示・スナップ・クリック開始。
  空きスペースクリックの無効化。
- **WP5 Command 無視モード**: Pin 打ち、赤表示、閉じ禁止、確定面の食い込み警告。
- **WP6 サブ円**: ツール、配置/削除、スライダー、参照中削除の拒否。
- **WP7 仕上げ**: 減算UIの完全削除、JSON v2 保存/読込、README.md の更新、
  全テストグリーン、`cargo clippy` の新規警告なし。

## 変えないこと

- geometry.rs のソルバー本体（update_thread / 接線関数 / ヒステリシス）は
  正しく動いているので、リファクタリングしない。追加のみ。
- 描画中は一本の線のみ表示、という原則。
- 日本語UI・ダークトーン・既存ショートカット。
