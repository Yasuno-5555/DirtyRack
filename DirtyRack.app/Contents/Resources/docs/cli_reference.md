# DirtyRack Forensic CLI Reference (Draft)

`dirtyrack-cli` は、決定論的なオーディオレンダリングと、パッチの同一性検証（Forensic Validation）を行うためのコマンドラインツールです。

## 基本コマンド

### `dirtyrack render <PATCH_JSON> [OPTIONS]`
指定されたパッチファイルをオフラインでレンダリングし、決定論的なオーディオファイルを出力します。
- `--output <FILE>`: 出力ファイル名 (.wav)。
- `--length <SEC>`: レンダリングする長さ。
- `--sample-rate <HZ>`: サンプルレート。
- **特徴**: レンダリング結果のハッシュ値が計算され、過去のテイクとの同一性が検証されます。

### `dirtyrack verify <PATCH_JSON> <HASH>`
パッチのレンダリング結果が、指定されたハッシュ値とビット単位で一致するかを検証します。CI/CD 環境での回帰テストに使用されます。

### `dirtyrack gui`
メインのグラフィカル・プロジェクター（GUI）を起動します。

## 開発者向けコマンド

### `dirtyrack module list`
現在ロード可能な内蔵モジュールおよびサードパーティ製モジュール（`modules/` フォルダ内）の一覧を表示します。

### `dirtyrack sdk init <DIR>`
新しいサードパーティ・モジュール開発のためのテンプレートプロジェクトを指定したディレクトリに作成します。

---

> [!NOTE]
> 現在、DirtyRack は GUI による直感的なパッチングを主軸としていますが、背後では常にこの CLI 相当の決定論的エンジンが動作し、すべての操作の再現性を担保しています。
