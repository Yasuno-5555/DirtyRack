# Tutorial: 初めての DirtyRack モジュール作成

このチュートリアルでは、入力を 2倍にするシンプルな「Gain」モジュールを作成しながら、DirtyRack エコシステムへの参加方法を学びます。

## Step 1: プロジェクトの作成

まず、新しい Rust ライブラリプロジェクトを作成します。

```bash
cargo new my-dirty-module --lib
cd my-dirty-module
```

## Step 2: Cargo.toml の設定

DirtyRack は動的ライブラリ（.so, .dll, .dylib）をロードします。また、SDK を依存関係に追加する必要があります。

`Cargo.toml` を開き、以下のように編集してください：

```toml
[package]
name = "my-dirty-module"
version = "0.1.0"
edition = "2021"

[lib]
# 重要: 動的ライブラリとしてビルドすることを指定します
crate-type = ["cdylib"]

[dependencies]
# 安定した SDK を使用します
dirtyrack-sdk = "0.1" 
```

## Step 3: コードの実装 (`src/lib.rs`)

`src/lib.rs` の中身をすべて消して、以下の「Gain モジュール」のコードを貼り付けてください。

```rust
use dirtyrack_sdk::*;

// 1. モジュールのデータ構造を定義
struct MyGainModule {
    // process ループ内でのメモリ確保は禁止なので、必要なバッファはここで持っておく
}

impl MyGainModule {
    pub fn new(_sample_rate: f32) -> Self {
        Self {}
    }
}

// 2. 音響処理ロジックの実装
impl RackDspNode for MyGainModule {
    fn process(
        &mut self,
        inputs: &[f32],      // 入力電圧の配列 (16ボイス分)
        outputs: &mut [f32], // 出力電圧の配列 (16ボイス分)
        params: &[f32],      // ノブの値の配列
        ctx: &RackProcessContext, // 経年劣化や個体差などの情報
    ) {
        let gain_knob = params[0];
        
        // DirtyRack は常に 16ch ポリフォニックです。
        for i in 0..16 {
            // ctx.imperfection からボイス固有の個体差を取得
            let p_offset = ctx.imperfection.personality[i] * 0.05;
            let gain = (gain_knob + p_offset).max(0.0);
            
            outputs[0 * 16 + i] = inputs[0 * 16 + i] * gain;
        }
    }
}

// 3. モジュールの「顔（記述子）」を定義
fn my_descriptor() -> &'static ModuleDescriptor {
    &ModuleDescriptor {
        id: "com.yourname.gain", // 世界でユニークなID
        name: "My First Gain",   // ブラウザに表示される名前
        manufacturer: "Independent Crafter",
        hp_width: 4,             // モジュールの横幅 (1HP = 5.08mm)
        
        // --- ここで見た目をカスタマイズ！ ---
        visuals: ModuleVisuals {
            background_color: [50, 60, 70], // 深みのあるブルーグレー
            text_color: [255, 255, 255],    // 白色のテキスト
            accent_color: [0, 255, 150],    // 鮮やかなエメラルドのアクセント
            panel_texture: PanelTexture::MatteBlack,
        },
        
        // パラメータ（ノブ）の定義
        params: &[
            ParamDescriptor {
                name: "GAIN",
                kind: ParamKind::Knob,
                response: ParamResponse::Immediate,
                min: 0.0, max: 2.0, default: 1.0,
                position: [0.5, 0.5], // フェースプレート上の位置 [x, y]
                unit: "x",
            },
        ],
        
        // 入出力ポートの定義
        ports: &[
            PortDescriptor { name: "IN", direction: PortDirection::Input, signal_type: SignalType::Audio, position: [0.5, 0.2] },
            PortDescriptor { name: "OUT", direction: PortDirection::Output, signal_type: SignalType::Audio, position: [0.5, 0.8] },
        ],
        
        // DirtyRack がモジュールを生成するための工場関数
        factory: |sr| Box::new(MyGainModule::new(sr)),
    }
}

// 4. DirtyRack 宇宙へのエクスポート
export_dirty_module!(my_descriptor);
```

## Step 4: ビルドとインストール

いよいよビルドです。必ず `--release` をつけて最適化を有効にしてください。

```bash
cargo build --release
```

ビルドが成功すると、`target/release/` フォルダの中にライブラリファイルが生成されます：
- macOS: `libmy_dirty_module.dylib`
- Linux: `libmy_dirty_module.so`
- Windows: `my_dirty_module.dll`

このファイルを、DirtyRack 本体の実行ファイルと同じ場所にある `modules/` フォルダ（存在しない場合は作成）にコピーしてください。

## Step 5: DirtyRack で確認

DirtyRack を起動し、「Add Module」ブラウザを開くと、あなたの作った **"My First Gain"** がリストに並んでいるはずです！

---

## 次のステップへのヒント

- **不完全さの調律**: `ctx.imperfection` を使い、ボイスごとに微妙に異なるキャラクターを与えましょう。
- **鑑識の透明性**: `get_forensic_data` を実装して、あなたのモジュールの「秘密」をユーザーがインスペクタで覗けるようにしましょう。
- **憲法**: 迷ったら `docs/SDK_Documentation.md` の「憲法」に立ち返りましょう。
