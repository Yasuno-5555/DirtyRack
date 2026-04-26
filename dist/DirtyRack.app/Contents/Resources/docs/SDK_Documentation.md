# DirtyRack Third-Party Module SDK Documentation

DirtyRack の「決定論的宇宙」へようこそ。この SDK は、ビット単位で再現可能なモジュラーシンセ・モジュールを開発するための道具箱です。

## クイックスタート

まずは[チュートリアル：初めてのモジュール作成](Tutorial_Creating_Modules.md)を読んで、5分でモジュールをビルドしてみましょう。

## 1. 開発の憲法 (The Constitution)

DirtyRack モジュールを書く際、以下の制約は絶対です：

- **完全な決定論**: 同じ入力とシードからは、常に同じ出力を生成せよ。`std::time` や `/dev/random` に頼るな。
- **NO-ALLOC**: `process` ループ内での `Vec`, `Box`, `HashMap` などの動的メモリ確保を禁止する。
- **libm の使用**: 数学関数には `std` の代わりに `libm` を使用せよ（プラットフォーム間の浮動小数点の挙動を一致させるため）。
- **Imperfection Integration**: `RackProcessContext` から提供される `ImperfectionData` を用い、16 ボイスそれぞれに適切な「揺らぎ」と「個性」を宿せ。
- **Forensic Transparency**: `get_forensic_data` を実装し、モジュールの内部状態を GUI に公開せよ。

## 2. コア・インターフェース

すべてのモジュールは `RackDspNode` トレイトを実装します。

```rust
pub trait RackDspNode: Send + Sync {
    fn process(
        &mut self,
        inputs: &[f32],
        outputs: &mut [f32],
        params: &[f32],
        ctx: &RackProcessContext,
    );

    /// 鑑識用データの報告 (Drift Inspector 用)
    fn get_forensic_data(&self) -> Option<ForensicData> { None }
    
    // ... その他の永続化用メソッド
}
```

### RackProcessContext
実行時の重要なメタデータが含まれます。
- `aging`: グローバルな経年劣化パラメータ (0.0..1.0)。
- `imperfection`: 16 ボイス分の `personality` (静的個体差) と `drift` (動的熱揺らぎ)。

## 3. 動的ロードの仕組み

DirtyRack は `get_dirty_module_descriptor` というシンボルを探します。`export_dirty_module!` マクロを使用して、自身を登録してください。

```rust
use dirtyrack_sdk::*;

struct MyModule { ... }
impl RackDspNode for MyModule { ... }

fn my_descriptor() -> &'static ModuleDescriptor {
    &ModuleDescriptor {
        id: "com.example.my_vco",
        name: "Super VCO",
        manufacturer: "Example Corp",
        hp_width: 10,
        params: &[ ... ],
        ports: &[ ... ],
        factory: |sr| Box::new(MyModule::new(sr)),
    }
}

export_dirty_module!(my_descriptor);
```

## 4. 推奨ワークフロー

1. `dirtyrack-sdk` を `Cargo.toml` に追加。
2. `crate-type = ["cdylib"]` を指定。
3. `cargo build --release` でビルド。
4. 生成された `.so`/`.dll`/`.dylib` を DirtyRack の `modules` フォルダに配置。

---

> [!IMPORTANT]
> 「一人の開発者の非決定性は、パッチ全体のハッシュを破壊する」。
> 常に再現性を意識してコードを記述してください。
