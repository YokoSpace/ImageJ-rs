# Java 1.54u と ImageJ-rs のベンチマーク

同じ 8-bit ramp スタックに対し、ImageJ 1.54u と Rust のコマンドを別プロセスで実行します。対象は `invert`（全スライス反転）、`gaussian`（sigma=2、全スライス）、`rotate90`（右に90°、全スライス）です。各実行の画像寸法と全画素のハッシュを照合し、一致しなければ計測を失敗として終了します。

```sh
python3 tools/benchmark/compare.py --out-dir verification/benchmarks/manual
```

既定値は 1024×768、4スライス、各操作5回、ウォームアップ2回、独立したプロセス3回です。`--width`、`--height`、`--slices`、`--iterations`、`--warmup`、`--runs`、`--cases` で変更できます。短い動作確認には `--profile dev --width 64 --height 48 --slices 2 --iterations 2 --warmup 1 --runs 1` を使えます。通常の比較は既定の `release` ビルドで行ってください。

標準出力に操作1回あたりの中央値（ms）、最大 RSS の中央値（MiB）、Java 時間 ÷ Rust 時間を表示します。`--out-dir` を指定すると、環境・条件・集計結果を `summary.json`、個別計測を `samples.csv` に保存します。各回で Java と Rust の起動順を交互に変えています。

処理時間は各ワーカー内で画像生成とウォームアップの**後**に測り、コマンドの呼び出しと画像処理を含みます。ビルド時間と起動時間は含みません。最大 RSS は `/usr/bin/time` が報告する**プロセス全体のピーク**で、ランタイム、画像生成、ウォームアップ、処理、結果のハッシュ計算を含みます。Java のヒープ容量だけとの比較ではありません。macOS と Linux に対応し、同じマシン・同じ条件での比較を想定しています。

Java 側は未変更の `original_imagej/` から作られるオラクル JAR と、ベンチマーク専用の [`BenchRunner.java`](BenchRunner.java) を使います。Rust 側は `ij-cli bench-worker` で `Session` の対応コマンドを実行します。ビルド・計測の起点は [`compare.py`](compare.py) です。
