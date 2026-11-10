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

アプリ内の **File → Open** からも開けます。画像ファイルなしで試すなら、**File → New → Image... (8-bit Ramp)** で画像を作り、矩形を選択して **Analyze → Measure** を実行すると Results ウィンドウに計測結果が出ます。TIFF リーダーは実装済み形式の非圧縮ファイルに対応しています。圧縮 TIFF には未対応です。開発中のアプリなので、大切な画像はコピーを保存してから編集してください。

## 現在できること

- ツールバー、独立した画像ウィンドウ、ズーム、Results ウィンドウ、ImageJ に合わせたステータス表示と画像情報表示。
- 8-bit グレースケール・インデックスカラー、16-bit グレースケール、32-bit float、RGB の画像データ。データモデルと TIFF の読み書きでは複数スライスと C/Z/T 次元にも対応。
- 矩形、楕円、直線、多角形、フリーハンド、Wand の選択領域。矩形・楕円・直線の ROI はマウスで移動やサイズ変更もできます。
- 表示範囲、Brightness & Contrast、しきい値、実装済みのフィルタと計測、TIFF の読み書き、対応コマンドのマクロ記録。
- 後述する**限定的な範囲**での従来の Java プラグイン実行。

アプリ内で **unsupported** と表示されるメニュー項目は実装予定です。Crop、Duplicate、画像型変換、Plot Profile、一部の計測設定、ハイパースタック操作などは未完成です。詳しくは [Phase 2 ロードマップ](docs/roadmap_phase2_ui_fidelity.md) を参照してください。

## 従来の ImageJ プラグイン

**Plugins → Installed Plugins** から従来の ImageJ プラグインディレクトリをスキャンし、項目を検査できます。現在実行できるのは、別の Java プロセスで動かす通常の `PlugInFilter` の一部です。入力は、既定のメタデータを持つ単一スライスの 8-bit グレースケール画像に限られます。ROI は無し、または画像内に収まる矩形のみ対応します。それ以外の画像、スタック、未対応の UI やホスト機能を必要とするプラグインは、暗黙に変換せずエラーにします。

プラグイン実行にセキュリティ上のサンドボックスはありません。信頼できるコードだけを実行してください。Java プラグイン全般への互換性、Plugins メニューからの直接実行、Rust プラグイン API は今後の課題です。

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
| [`crates/ij-io/`](crates/ij-io/) | TIFF の読み書き |
| [`crates/ij-session/`](crates/ij-session/) | 画像、コマンド、計測結果、マクロ記録 |
| [`crates/ij-app/`](crates/ij-app/) | デスクトップアプリ |
| [`crates/ij-bridge/`](crates/ij-bridge/) | Java オラクルとプラグインのプロセス間連携 |
| [`original_imagej/`](original_imagej/) | 読み取り専用の ImageJ 参照ソース |
| [`tools/`](tools/) | オラクル、プラグインホスト、検証用スクリプト |

設計については [アーキテクチャの説明](docs/architecture_rationale.md) を参照してください。挙動を変更する場合は ImageJ 1.54u と比較し、`./tools/verify.sh` を実行してください。`original_imagej/` を編集しないなどの開発ルールは [AGENTS.md](AGENTS.md) にあります。

## 出典とライセンス

ImageJ は米国 NIH で開発されたパブリックドメインのプロジェクトです。詳しくは [`original_imagej/LICENSE.txt`](original_imagej/LICENSE.txt) を参照してください。Rust の各クレートは [`Cargo.toml`](Cargo.toml) で `MIT OR Apache-2.0` を宣言しています。
