# ImageJ-rs

[English](README.md)

**ImageJ-rs は、ImageJ 1.54u を Rust で再現する開発中のプロジェクトです。** ImageJ に慣れた人が使いやすいウィンドウ構成と操作感を保ちながら、画像処理や計測結果をできるだけ一致させることを目指しています。[`original_imagej/`](original_imagej/) の Java 実装を仕様の基準とし、Rust の結果を自動テストで比較しています。

現時点では ImageJ や Fiji の完全な代替ではありません。配布用のリリースはまだなく、ソースからビルドします。

## デスクトップアプリを試す

新しい Rust ツールチェーン、Python 3、`zsh`、および `java`・`javac`・`jar` を含む JDK が必要です。現在の開発・検証環境は macOS と JDK 27 です。ほかの OS や JDK バージョンは未検証です。

```sh
git clone https://github.com/YokoSpace/ImageJ-rs.git
cd ImageJ-rs
./tools/oracle/build_oracle.sh
cargo run --release -p ij-app
```

起動時に TIFF を開く場合:

```sh
cargo run --release -p ij-app -- /path/to/image.tif
```

アプリ内の **File → Open** から TIFF、PNG、JPEG を開けます。画像ファイルなしで試すなら、**File → New → Image... (8-bit Ramp)** で画像を作り、矩形を選択して **Analyze → Measure** を実行すると Results ウィンドウに計測結果が出ます。TIFF リーダーは対応する画像形式の PackBits、Deflate、LZW 圧縮も読み込めます。開発中のアプリなので、大切な画像はコピーを保存してから編集してください。

## 現在できること

- ツールバー、独立した画像ウィンドウ、ズーム、Results ウィンドウ、ImageJ に合わせたステータス表示と画像情報表示。
- 8-bit グレースケール・インデックスカラー、16-bit グレースケール、32-bit float、RGB の画像データ。データモデルと TIFF の読み書きでは複数スライスと C/Z/T 次元にも対応。
- 矩形、楕円、直線、多角形、フリーハンド、Wand、ポイント、角度、折れ線、フリーハンド線の選択領域。ROI Manager では選択領域の登録、選択、削除、計測ができます。
- 表示範囲、Brightness & Contrast、ヒストグラムを伴うしきい値のライブ表示、実装済みのフィルタと計測、TIFF の読み書き、対応コマンドのマクロ記録。
- Crop、Duplicate、8-bit/16-bit/32-bit/RGB の画像型変換、Set Measurements、Set Scale、Histogram、Plot Profile、14 種類の Process → Math 演算。対応するフィルタと演算はスタックにも適用できます。
- TIFF、PNG、JPEG の読み書きと Results 表の保存。対応する可逆形式の PNG と TIFF では画素を保持し、JPEG は非可逆圧縮で保存します。
- ハイパースタックの C/Z/T 操作、スライスのアニメーション再生、ImageJ に合わせた macOS のネイティブメニューバー（ほかの OS ではウィンドウ内メニュー）。
- 後述する**限定的な範囲**での従来の Java プラグイン実行と、Rust 製画像プラグイン用 API。

現在の主な残課題は、Edit の描画・クリップボード操作、二値化、Image Calculator、Analyze Particles、Text と Color Picker ツールです。残る一連の操作は [Phase 3 ロードマップ](docs/roadmap_phase3_practical_workflows.md) に、以前の UI 対応は [Phase 2 ロードマップ](docs/roadmap_phase2_ui_fidelity.md) にまとめています。

## 従来の ImageJ プラグイン

**Plugins → Installed Plugins** から従来の ImageJ プラグインディレクトリをスキャンし、項目を検査できます。スキャンと検査が完了したカタログはキャッシュされ、互換性のある項目は **Plugins** メニュー直下からワンクリックで実行できます。対応する通常の `PlugInFilter` は別の Java プロセスで実行し、8-bit、16-bit、32-bit、RGB 画像と通常のスタックを扱えます。`DOES_STACKS` を宣言するフィルタはスタック全体を処理できます。数値、文字列、選択肢、チェックボックスからなる標準的な `GenericDialog` 1 個は egui で表示できます。インデックスカラー画像、ハイパースタック、矩形以外の ROI、高度なフィルタフラグ、複雑なダイアログにはまだ対応していません。

プラグイン実行にセキュリティ上のサンドボックスはありません。信頼できるコードだけを実行してください。[`ij-plugin-api` クレート](crates/ij-plugin-api/) には、画像と Results の変更を成功時だけ反映する型付きの Rust 製画像プラグイン API があります。

## 実装を検証する

```sh
./tools/verify.sh
```

このコマンドは Java の参照ソースの整合性を確認し、ImageJ 1.54u のオラクルをビルドし、Java と Rust の結果比較を含むテストと、TIFF → ROI → 計測 → 保存の一連の操作を実行します。成功時の最終行は `VERIFY_ALL_OK` です。検証結果は `verification/evidence/latest/` に保存されます。

環境だけを簡単に確認する場合:

```sh
cargo run -p ij-cli -- doctor
```

`ij-cli` は検証用であり、ImageJ の汎用 CLI ではありません。現在のコマンドは `doctor`、`slice1`、`parity-check` です。

## リポジトリ構成

| パス | 役割 |
| --- | --- |
| [`crates/ij-core/`](crates/ij-core/) | I/O に依存しない画像・ROI・計測・処理ロジック |
| [`crates/ij-io/`](crates/ij-io/) | TIFF、PNG、JPEG の読み書き |
| [`crates/ij-session/`](crates/ij-session/) | 画像、コマンド、計測結果、マクロ記録 |
| [`crates/ij-app/`](crates/ij-app/) | デスクトップアプリ |
| [`crates/ij-bridge/`](crates/ij-bridge/) | Java オラクルとプラグインのプロセス間連携 |
| [`crates/ij-plugin-api/`](crates/ij-plugin-api/) | 型付き Rust 製画像プラグインの API |
| [`original_imagej/`](original_imagej/) | 読み取り専用の ImageJ 参照ソース |
| [`tools/`](tools/) | オラクル、プラグインホスト、検証用スクリプト |

設計については [アーキテクチャの説明](docs/architecture_rationale.md) を参照してください。挙動を変更する場合は ImageJ 1.54u と比較し、`./tools/verify.sh` を実行してください。`original_imagej/` を編集しないなどの開発ルールは [AGENTS.md](AGENTS.md) にあります。

## 出典とライセンス

ImageJ は米国 NIH で開発されたパブリックドメインのプロジェクトです。詳しくは [`original_imagej/LICENSE.txt`](original_imagej/LICENSE.txt) を参照してください。Rust の各クレートは [`Cargo.toml`](Cargo.toml) で `MIT OR Apache-2.0` を宣言しています。
