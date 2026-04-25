# DirtyData CLI Reference

`dirtydata-cli` は、システムと人間をつなぐ唯一の公式なインターフェースです。
「すべての状態は説明可能か、さもなくば破棄可能でなければならない」という哲学のもと、様々なコマンドが提供されています。

## 基本コマンド

### `dirtydata init`
現在のディレクトリを DirtyData プロジェクトとして初期化します。
`.dirtydata/` ディレクトリが生成され、デフォルトの `main` ブランチが作成されます。

### `dirtydata status`
現在のグラフ状態、ノード/エッジ数、直近のパッチ履歴、Active Intent、およびシステムの「信頼性スコア (Confidence Score)」を視覚的に表示します。
このコマンドは、プロジェクトがどの程度「正しく説明可能な状態か」を確認するための最重要コマンドです。

### `dirtydata doctor`
プロジェクトの健康状態を診断し、エラーや負債（Confidence Debt）、および「破棄可能な（Disposable）ノード」を検出して警告します。
問題が見つかった場合、それを修復するためのサジェスト機能も備えています。

## パッチ操作

DirtyData では、状態の変更はすべてパッチを通じて行われます。

### `dirtydata patch apply <FILE> [--intent <INTENT_ID>]`
JSON 形式のパッチファイルを適用します。
内部的には、`UserAction` を `Operation` にコンパイルし、グラフの Revision を進め、現在のブランチの HEAD を更新します。
`--intent` オプションを指定することで、パッチの適用理由（意図）を明記することができます。

### `dirtydata patch list`
現在のブランチに適用されているパッチの履歴を一覧表示します。

### `dirtydata patch replay [--verify]`
現在の履歴に記録されているすべてのパッチを最初から再生（リプレイ）し、最終的な状態が現在のグラフと完全に一致するか（決定論的か）を検証します。
`--verify` を付けると BLAKE3 ハッシュベースの厳密な比較が行われます。

## タイムラインとブランチ (Timeline)

### `dirtydata branch <NAME>`
現在の状態（HEAD）から新しいブランチをフォークします。

### `dirtydata checkout <NAME>`
指定したブランチに切り替えます。
物理ファイルのコピー等は一切発生せず、IR のポインタをスワップするだけで瞬時に別の状態へ遷移します。

## デーモンと監視 (Observer & Runtime)

### `dirtydata daemon`
プロジェクトディレクトリの変更監視と、リアルタイムのオーディオ再生（cpal）をバックグラウンドで開始します。
- JSONパッチが適用された瞬間に、音を止めずにグラフをホットリロードします。
- プラグインがクラッシュした場合は、ログを出力して無音バッファへフォールバックします。

### `dirtydata observe`
外部ファイルシステム（WAV ファイル等）のハッシュやタイムスタンプを再計算し、`.dirtydata/observations.json` を更新します。

### `dirtydata repair <NODE_NAME>`
Observer によって検知された「意図しないハッシュの不一致」に対し、現在の外部ファイルの状態を「正しい」ものとして再定義（修復パッチを適用）します。

### `dirtydata gui`
グラフィカル・プロジェクターを起動します。
- **投影**: 現在の IR グラフ、接続関係、ノードパラメーター、Intent Zone、Confidence 状態を視覚化します。
- **干渉**: マウス操作によるノード移動や接続組み換え、パラメーター調整が可能です（楽観的描画による低遅延フィードバック付き）。
- **同期**: 表示設定は `ui_layout.json` に保存され、Core の真実は自動的にパッチとして永続化されます。

## 意図の管理 (Intent)

### `dirtydata intent add <DESCRIPTION> [--must <...>] [--prefer <...>]`
新しい Intent（意図・制約）をシステムに登録します。

### `dirtydata intent list`
現在システムに登録されている Intent の一覧を表示します。

### `dirtydata intent attach <INTENT_ID> <PATCH_ID>`
既存のパッチに Intent を紐付けます（「なぜこの操作をしたのか」を後付けで説明します）。

## オーディオ出力

### `dirtydata render [--output <FILE>] [--length <SEC>] [--sample-rate <HZ>]`
現在のグラフをオフラインでレンダリング（Deterministic Bounce）し、WAV ファイルとして出力します。
出力されたファイルには SHA-256 ハッシュが付与され、その決定論的な同一性が保証されます。
