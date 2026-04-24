# DirtyData

**Deterministic Creative Operating System**

DirtyData は、音楽・映像・ゲーム・ポストプロダクションなど、複数の時間軸と非決定的な外部要素を含む創作環境に対して、**因果関係・意図・状態遷移を追跡可能にするための Deterministic Creative Operating System** です。

単なる「編集ツール（DAW）」ではなく、制作という本質的にカオスな行為に対して「説明責任（Accountability）」を与える Runtime + Provenance System として設計されています。

## 核心的設計思想

* **実験の自由と出荷の厳格性を両立する**
* **GUI・コード・実行結果の不一致を解消する**
* **「何をしたか」ではなく「なぜそうなったか」を保存する**
* **ブラックボックスな外部プラグインを検疫しながら共存する**
* **思考の曖昧さを保護しつつ、最終成果物の再現性を保証する**

詳細な思想については [Design Philosophy](docs/design_philosophy.md) を参照してください。

## 主な機能 (Phase 3 Core Features)

1. **Timeline & Branching (Git + DAW Hybrid)**
   物理ファイルを複製することなく、Git のようにパラレルワールド（ブランチ）を作成し、IR ポインタの切り替えによって超高速に状態を行き来できます。
2. **Playable System (cpal + arc-swap)**
   CLI からのパッチ適用によって、ロックフリー・ダブルバッファリングを用いたオーディオエンジンの DSP グラフが音切れなしで瞬時に切り替わります。
3. **Plugin Boundary (IPC Sandbox)**
   不確実性の高い外部VSTプラグインを安全な別プロセス（Sandbox）に隔離。NaN Storm やクラッシュ発生時には、DAWごと道連れにすることなく、瞬時に安全な Frozen Asset にフォールバックします。
4. **Observer Daemon**
   ファイルシステムと外部アセットを常時監視。ハッシュの自動再計算を行い、手動でのファイル改ざんなど「疑わしい変更」を UI 上で視覚化し警告します。
5. **Graphical Projector (Phase 4 — egui)**
   「GUI は Core IR の投影である」という理念に基づく視覚インターフェース。オーディオエンジンを妨げない Shadow Graph 同期、楽観的描画による低遅延操作、そして Intent や Confidence Score の視覚化を実現します。

詳細な技術構造については [Architecture](docs/architecture.md) を参照してください。

## Getting Started

### 必須要件
- Rust (Edition 2021)
- Cargo

### インストールとビルド

```bash
git clone https://github.com/your-org/dirtydata.git
cd dirtydata
cargo build --release
```

### プロジェクトの初期化

DirtyData プロジェクトを作成したいディレクトリで以下を実行します。

```bash
cargo run --bin dirtydata-cli -- init
```
`.dirtydata/` 隠しディレクトリが生成され、`main` ブランチが初期化されます。

### サウンドの再生と監視（デーモンの起動）

別ターミナルでデーモンを起動し、オーディオ再生と監視を開始します。

```bash
cargo run --bin dirtydata-cli -- daemon
```
*オーディオデバイスが立ち上がり、パッチの適用を待ち受けます。*

### グラフィカル・プロジェクターの起動

Core の状態を視覚化し、操作するための GUI を起動します。

```bash
cargo run --bin dirtydata-cli -- gui
```
*egui ベースのインターフェースが立ち上がり、IR (current.json) の変更をリアルタイムに投影します。*

### パッチの適用（ホットリロード）

デーモンを起動したまま、パッチファイル（JSON）を適用してグラフを構築します。

```bash
# 基本的なサイン波 -> ゲイン -> 出力のチェインを構築
cargo run --bin dirtydata-cli -- patch apply examples/basic_chain.json

# 音を止めずに、EQ（ゲイン追加）をホットリロードで挿入
cargo run --bin dirtydata-cli -- patch apply examples/add_eq.json
```

### ブランチを使った実験

音のバリエーションを試すために、ブランチを切って実験します。

```bash
# 新しいアイデアのためのブランチを作成・移動
cargo run --bin dirtydata-cli -- branch heavy_bass
cargo run --bin dirtydata-cli -- checkout heavy_bass

# 実験的なパッチを適用
cargo run --bin dirtydata-cli -- patch apply examples/heavy_bass.json

# 気に入らなければ、元の main ブランチへ一瞬で戻る（音も瞬時に戻ります）
cargo run --bin dirtydata-cli -- checkout main
```

詳細なコマンド群については [CLI Reference](docs/cli_reference.md) を参照してください。

## プロジェクト構造

```text
DirtyData/
├── crates/
│   ├── dirtydata-core/     # 核心となる IR (Graph, Node, Edge), Patch, Storage 定義
│   ├── dirtydata-observer/ # 外部世界を監視し「疑い」を定量化する Observer 層
│   ├── dirtydata-intent/   # ユーザーの「意図」をグラフに制約として紐付ける Intent 層
│   ├── dirtydata-host/     # 不安定なプラグインを隔離する IPC サンドボックス層
│   ├── dirtydata-runtime/  # Playable なリアルタイム・オーディオ・ストリーム層
│   ├── dirtydata-gui/      # 「投影」と「干渉」を司る egui 視覚層
│   └── dirtydata-cli/      # 人間とシステムを繋ぐ最初の接点
├── docs/                   # ドキュメント一式
└── examples/               # テスト用パッチファイル群
```

## ライセンス

[MIT License](LICENSE) (想定)
