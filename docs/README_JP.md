# DirtyRack

**Deterministic Eurorack Simulator & Modular DSP Engine**

DirtyRack は、ビット単位の再現性と物理的なモジュラー操作を融合させた、**高精度決定論的ユーロラック・シミュレータ**です。アナログの「カオス」や「揺らぎ」を愛しながらも、デジタルの「説明責任（Accountability）」を追求するアーティストとエンジニアのために設計されました。

単なるシンセサイザーではなく、パッチの全状態、演奏のジェスチャー、そして時間の流れを完全に固定し、ハッシュ値で検証可能にする **Forensic Audio Engine** です。

## 核心的設計思想

*   **決定論的カオス**: カオスアトラクタや非線形フィードバックを扱いながら、同一シード・同一入力からは常に 1bit の狂いもない出力を保証する。
*   **Massive Polyphony (16ch)**: VCV Rack 互換の 16 チャンネル・ポリフォニック・ケーブルを標準搭載。一本の結線がオーケストラのような密度を生む。
*   **Audio Sanctity (聖域)**: オーディオ計算スレッドは 100% ロックフリー。UI 負荷やファイル IO によって音が途切れることを物理的に許さない。
*   **Forensic Observation**: 「なぜこの音になったのか」を証明する鑑識レイヤー。Drift Inspector により、内部の熱状態や個体差をリアルタイムに解剖可能。
*   **Open Ecology**: サードパーティが `dirtyrack-sdk` を用いて、独自の決定論的モジュールを Rust で開発・配布可能。

## 主な機能

1.  **Massive Polyphonic DSP**: 全モジュールが 16 ボイス独立処理に対応。一本のケーブルでポリフォニックな表現を完結。
2.  **Analog Imperfection Layer**: 決定論的に再現される「機材の個性（Personality）」と「熱ドリフト」。アナログ特有の不安定さを科学的にエミュレート。
3.  **Aging Knob**: 新品の輝きから 20 年物の退廃までを、グローバルな「経年劣化」ノブ一つで制御。
4.  **Forensic Inspector**: モジュールの内部状態を詳細に分析。ピッチのズレやフィルターの飽和の原因を客観的データとして可視化。
5.  **Triple-Buffer Visuals**: オーディオスレッドの神聖さを保ちつつ、60fps+ の滑らかな波形投影と LED レベル表示を実現。
6.  **MIDI-CV Bridge**: 外部 MIDI 信号をポリフォニックな 1V/Oct 信号へと変換し、ハードウェアとソフトウェアの境界を消失させる。
7.  **Deterministic Auditing (New)**: サンプル精度の「決定論の破れ」を検知する Divergence Map と、音の因果を遡る Intent-to-Sound Trace 機能を搭載。

### Distribution & Formats

- **Standalone App**: macOS 用 `.app` バンドルを提供。`/Applications` にドラッグ＆ドロップで即座に利用可能。
- **VST3 / CLAP Plugin**: DAW (Ableton, Bitwig, Reaper, etc.) 内で 16ch ポリフォニック・モジュラーとして動作。
- **CLI Tool**: 決定論的なオフラインレンダリングとハッシュ検証のためのコマンドライン・インターフェース。

## クイックスタート

### Standalone (macOS)
```bash
# ルートの DirtyRack.app を /Applications にコピー
open ./DirtyRack.app
```

### Plugin Deployment
ルートに生成された `DirtyRack.clap` をプラグインフォルダに配置してください。

**VST3 として使用する場合**:
1. `DirtyRack.vst3/Contents/MacOS/` というディレクトリ構造を作成。
2. その中に `DirtyRack.clap` を `DirtyRack` という名前でコピーして配置してください。

## プロジェクト構造

```text
DirtyRack/
├── crates/
│   ├── dirtyrack-sdk/      # サードパーティ開発用 SDK。コア・トレイトと SIMD ユーティリティ。
│   ├── dirtyrack-modules/  # 決定論的 DSP モジュール兵器廠（VCO, VCF, Chaos, etc.）
│   ├── dirtyrack-gui/      # egui ベースの「プロジェクター」。Triple-Buffer 同期。
│   └── dirtyrack-core/     # 決定論的基盤。因果関係を管理する DAG エンジン。
├── docs/                   # SDK ドキュメント、アーキテクチャ、哲学。
└── modules/                # サードパーティ製動的ライブラリ (.so, .dll) の配置場所。
```

## ドキュメント

- [Design Philosophy (設計哲学)](docs/design_philosophy.md)
- [Architecture (アーキテクチャ)](docs/architecture.md)
- [SDK Documentation (開発者ガイド)](docs/SDK_Documentation.md)
- [Creating Your First Module (チュートリアル)](docs/Tutorial_Creating_Modules.md)

## ライセンス

MIT License
