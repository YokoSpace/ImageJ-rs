# Java 1.54u と ImageJ-rs のベンチマーク

同じ決定的な入力に対し、ImageJ 1.54u と Rust のコマンドを別プロセスで実行します。対象は `invert`（全スライス反転）、`gaussian`（全スライス、sigma 可変）、`rotate90`（右に90°、全スライス）です。各実行の画像寸法・ビット深度・全画素のハッシュを照合し、一致しなければ計測を失敗として終了します。異なる仕事の時間を比較してしまうことはありません。

```sh
# 既定: 1024x768、4スライス、8-bit noise、sigma=2、各操作5回、ウォームアップ2回、独立したプロセス3回
python3 tools/benchmark/compare.py --out-dir verification/benchmarks/manual

# 16-bit、sigma=8.5、Java を 1 スレッドに固定
python3 tools/benchmark/compare.py --type 16 --sigma 8.5 --java-threads 1

# 旧ベースライン（8-bit ramp、sigma=2）の再現
python3 tools/benchmark/compare.py --input ramp

# 短い動作確認
python3 tools/benchmark/compare.py --profile dev --width 64 --height 48 --slices 2 \
    --iterations 2 --warmup 1 --runs 1
```

`--width`、`--height`、`--slices`、`--iterations`、`--warmup`、`--runs`、`--cases`、`--profile`、`--type`、`--sigma`、`--input`、`--seed`、`--java-threads`、`--rust-threads`、`--out-dir` を指定できます。通常の比較は既定の `release` ビルドで行ってください。

## 入力

`--input` は 3 種類で、いずれも両エンジンが同じ画素を作ります。

| 値 | 内容 |
| --- | --- |
| `noise`（既定） | xorshift64\* の種付きノイズ。1 画素につき 1 回抽選し、スライス順・行順に並べる |
| `impulse` | 同じ乱数列で 512 画素に 1 個だけ最大値を置く。それ以外は 0 |
| `ramp` | `NewImage` の ramp フィル。旧ベースラインの再現用 |

Java の `noise` フィルは `java.util.Random` を非シードで使うため再現できません。そこで乱数は [`BenchRunner.java`](BenchRunner.java) と [`crates/ij-cli/src/benchmark.rs`](../../crates/ij-cli/src/benchmark.rs) に同じ算術で実装しています（Java の `long` はラップし、`>>>` は論理シフトなので Rust の `u64` と一致します）。`--type` は `8`、`16`、`32` のグレースケールに対応し、`--seed` は `noise` と `impulse` に効きます。

既定の入力は以前の `ramp` から `noise` に変わりました。ぼかしの計算量は画素値に依存しないので時間の比較は変わりませんが、滑らかな画像では出力の差が見えにくいためです。旧ベースラインと同じ入力にするには `--input ramp` を指定してください。指紋の形式はビット深度を含むように変わったので、以前の指紋の値とは比較できません。

## スレッド数の公平性

- Java 側は `Prefs.setThreads(n)` で明示的に設定します。`--java-threads default`（既定）は何も設定せず、`Prefs.getThreads()` が解決した値（`IJ_Prefs.txt` が無ければ `Runtime.availableProcessors()`）を記録します。
- Rust 側は `ij-core` の worker budget をそのまま報告します。`--rust-threads` を省略すると `Session` の既定（`0` → `std::thread::available_parallelism`、`filters.rs::resolve_workers`）を使い、`policy` は `session-default` になります。`--rust-threads N` を指定すると `Session::dispatch` ではなく `ImageDocument::gaussian_blur_stack_with_workers(sigma, N)` を直接呼び、`policy` は `explicit-workers` になります。`invert` と `rotate90` は両エンジンとも逐次処理なので `resolved` は `null`、`policy` は `sequential` です。
- 報告する `resolved` は**ぼかしに与えた worker budget** であり、実際に走った OS スレッド数ではありません。`blur_one_direction` は行数と ImageJ のタスクサイズ推定で上限を掛けるため（GaussianBlur.java 273-277）、小さい画像では budget より少ないスレッドで動きます。この上限は `ij-core` の外からは観測できないので、数値ではなく仕様として記録しています。Java 側の `Prefs.getThreads()` も同じく上限値です。
- スレッド数は Java と Rust で**比較しません**。それぞれの実行環境の性質であり、同じ仕事をした証拠ではないためです。実測値は `summary.json` と `samples.csv` に残ります。
- worker budget は画素を変えません。`--rust-threads 1` と既定値の fingerprint が一致することをテストで確認しています。

Javaはスライスを順番に処理し、その内部の行・列を並列化します。Rustは上限内でスライス間にもワーカーを配分します。同じ budget でも処理の分け方は異なります。行・列だけの並列化を比較するには `--slices 1` を使ってください。

### 直接 core を呼ぶ経路について

`Command::GaussianBlurStack` は sigma しか運ばないため、worker budget を指定する場合だけ `ImageDocument::gaussian_blur_stack_with_workers` を直接呼びます。`Session` を通らないのでマクロ記録と dirty 更新は行われませんが、ぼかし本体は同じ `gaussian_blur_plane` を通ります。直接呼ぶ経路の計測には Session の処理時間が含まれないため、小さい画像では比較に注意してください。`--rust-threads` を省略した既定の計測は、通常の `Session::dispatch` 経路です。

スタックの同時処理は最大4スライスで、一時バッファの推定量に応じてさらに制限します。推定64 MiBを超える画像は1スライスずつ処理し、その中の行・列を並列化します。この値は全体のメモリ上限ではなく、最低1スライスの作業領域は確保します。列の並列処理には最大で画像1枚分の追加 `f32` 領域が必要です。

## メモリ（プロセス RSS）

`/usr/bin/time` が報告する**プロセス全体のピーク RSS** です。ランタイム、画像生成、ウォームアップ、処理、結果のハッシュ計算を含みます。Java と Rust を同じマシン・同じ条件で比較できますが、差には JVM や GC によるヒープ拡張も含まれます。処理アルゴリズム単独のメモリ効率を表す値ではありません。同じエンジンの変更前後の比較にも使えます。macOS と Linux に対応します。

処理時間は各ワーカー内で画像生成とウォームアップの**後**に測り、コマンドの呼び出しと画像処理を含みます。ビルド時間と起動時間は含みません。

Javaのコマンド表と設定は計測前に初期化し、ウォームアップ用画像も解放します。`--warmup 0` は演算のウォームアップを省略する指定です。プラグインの初回読み込みやJITの影響は残るため、継続使用時の比較には複数回のウォームアップを指定してください。

## 指紋（fingerprint）

FNV-1a 64 で、定義した生の表現だけをハッシュします。

1. ヘッダ: width、height、スライス数、ビット深度をそれぞれリトルエンディアンの `u32` として。
2. 画素: 格納値のリトルエンディアンバイト列。8-bit は `u8`、16-bit は `u16`、32-bit は `f32::to_bits`（Java 側は `Float.floatToRawIntBits`）の `u32`。

浮動小数は生ビットで比較するため、`-0.0` と `0.0` は不一致、NaN はペイロードまで比較します。入力は NaN も無限大も作らず（`noise` は `[0, 256)` の 1/256 刻みの厳密値）、有限値のガウスぼかしも有限に留まるので、出力に NaN が現れた時点でそれはパリティの失敗です。

照合する項目は `case`、`type`、`sigma`、`input`、`seed`、`width`、`height`、`slices`、`iterations`、`fingerprint` です。スレッド数と `machine_threads` は記録のみで照合しません。

## 出力

標準出力に操作1回あたりの中央値（ms）、最大 RSS の中央値（MiB）、Java 時間 ÷ Rust 時間、Java と Rust の実効スレッド数を表示します。`--out-dir` を指定すると次を保存します。

- `summary.json`: `settings`（全パラメータ）、`environment`（OS、Java、rustc、git commit と dirty 状態）、`summary`（case ごとの中央値とスレッド数）、`samples`（個別計測）
- `samples.csv`: `case, run, engine, type, sigma, input, seed, width, height, slices, iterations, elapsed_ns, peak_rss_mib, threads_requested, threads_resolved, threads_policy, machine_threads, fingerprint`

各回で Java と Rust の起動順を交互に変えています。

## テスト

```sh
cd tools/benchmark
python3 -m unittest test_compare

# Java と Rust を実際に走らせるスモークテスト（オラクルと release ビルドを作るため任意実行）
IJ_BENCH_SMOKE=1 python3 -m unittest test_compare.JavaRustSmokeTests
```

スモークテストは 3 case × 3 type × 3 input を 32×24×2 で実行し、指紋の一致だけを検証します。時間の assert はしないので、通常の CI に性能判定を持ち込みません。

## 実装

Java 側は未変更の `original_imagej/` から作られるオラクル JAR と、ベンチマーク専用の [`BenchRunner.java`](BenchRunner.java) を使います。Rust 側は `ij-cli bench-worker` で `Session` の対応コマンドを実行します。ビルド・計測の起点は [`compare.py`](compare.py) です。
