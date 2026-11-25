# Phase 4 ロードマップ: 完全性と高度な画像ワークフロー

基準は ImageJ 1.54u (`original_imagej/`)。Phase 3 の完了点は `87706ff`。本書は未対応機能を Java の挙動に照らして段階的に実装する計画であり、全 ImageJ/Fiji 機能の互換性を宣言するものではない。

## 0. 共通ルール

Phase 2 の [共通ルール](roadmap_phase2_ui_fidelity.md#0-全タスク共通ルール必ず読む) と Phase 3 の [停止条件](roadmap_phase3_practical_workflows.md#01-停止条件) を引き継ぐ。各小単位で次を守る。

1. 該当する `original_imagej/ij/` の Java 実装を先に読み、境界値をオラクルで観測する。推測した丸め、補間、ヒストグラム範囲を仕様にしない。
2. `crates/ij-session/tests/oracle_parity.rs` に Java/Rust の差分テストを先に追加する。UI は Phase 2 §0.4 の純粋関数・`UiProbe`・ヘッドレス egui テストを使う。期待する理由で失敗したことを確認する。
3. 最小の実装を行い、`ij-core` に I/O、UI、サブプロセスを入れない。`original_imagej/` は編集しない。台帳の生成物は手で編集せず、分類変更時に `python3 tools/ledger/sync_ledger.py` を実行する。
4. `./tools/verify.sh` が終了コード 0 で `VERIFY_ALL_OK` を出してから、その小単位をコミットし次へ進む。統合後の `main` でも再実行する。
5. 報告は `TASK / STATUS / CHANGED / NEW TESTS / VERIFY / NOTES` とし、Java 側の根拠と制限を記す。

並列作業は独立した worktree で行う。同じメニュー表・`AppAction`・`oracle_parity.rs` を変更するタスクは統合時に差分を確認し、テストを残して手で衝突を解決する。ブランチはマージと検証の後に整理する。

## 1. Wave 1: 基本操作の不足を埋める

| ID | 作業と Java の参照元 | 完了条件 |
| --- | --- | --- |
| P4.1a | 32-bit float とスタック全体の `Threshold > Auto`。`ij/plugin/frame/ThresholdAdjuster.java`, `ij/process/AutoThresholder.java`, `ij/process/FloatProcessor.java` | NaN、表示範囲、各スライス、全スタック集計の規則を Java と比較し、同じしきい値と適用結果を得る。 |
| P4.1b | `File > New > Image...`。`ij/gui/NewImage.java`, `ij/gui/GenericDialog.java` | 名前、型、塗りつぶし、幅、高さ、スライス数、既定値、Cancel、マクロ記録が一致する。P4.1a の `VERIFY_ALL_OK` 後に着手する。 |
| P4.2a | `Image > Transform > Flip Horizontally/Vertically`, `Rotate 90 Degrees Right/Left`。`ij/plugin/filter/Transformer.java`, `ij/process/*Processor.java` | 8/16/32-bit/RGB と ROI・スタックの Java 差分テストで画素、寸法、Undo が一致する。 |
| P4.2b | `Image > Transform > Rotate...` と `Image > Adjust > Size...`。`ij/plugin/filter/Rotator.java`, `ij/plugin/Resizer.java`, `ij/plugin/Scaler.java`（記録処理）, 各 Processor | 補間、背景、キャンバス、寸法・校正、Cancel とマクロ引数を Java と比較する。P4.2a の `VERIFY_ALL_OK` 後に着手する。 |
| P4.3a | `Edit > Selection > Create Mask / Create Selection / Clear Outside`。`ij/plugin/Selection.java`, `ij/plugin/filter/Filler.java` | 二値・しきい値・非矩形 ROI の各規則と元画像への影響が一致する。対象が複数 ROI 型へ広がる場合は小単位に分ける。 |

P4.1 は Devin SWE-2、P4.2 は Mistral Vibe `ds-flash`、P4.3 は Antigravity で並行する。各担当は Java の根拠、失敗テスト、最終検証結果とコミットを提出する。ブランチ整理は統合担当が実施する。

Wave 1 の P4.1a/b、P4.2a/b、P4.3a は実装・Java 差分テスト済み。制限: 16/32-bit の白画像の表示 LUT 反転、複数領域・穴を含む ShapeRoi、Size のハイパースタック深さ変更は未対応。詳細は各差分テストと実装を参照。

## 2. 後続 Wave

| Wave | ID | 対象と順序 |
| --- | --- | --- |
| 2 | P4.4, P4.5 | `Image > Stacks` の Z Project、スライス追加・削除、画像とスタックの相互変換。その後 `Image > Color` のチャンネル分離・結合、LUT 反転。 |
| 3 | P4.6, P4.7 | `Enhance Contrast...` と `Find Maxima...`、続いて `Subtract Background...` の Rolling Ball。各ダイアログの既定値とスタック規則も照合する。 |
| 4 | P4.8, P4.9 | `Process > FFT / Inverse FFT` と `Analyze > Calibrate...`、Results の `Summarize`。周波数データと濃度校正メタデータの保持を別々に検証する。 |
| 5 | P4.10, P4.11 | Java `PlugInFilter` への非矩形 ROI・indexed color・hyperstack の受け渡し、その後 `.ijm` の実行・編集。既存 Java マクロを利用する経路も評価する。 |
| 6 | P4.12, Final | BMP/GIF/Raw/Text Image と連番画像 I/O。最後にワークフロー全体の Java 差分、README 更新、`VERIFY_ALL_OK` を確認する。 |

後続 Wave の着手前に各機能の Java 実装、既存 API、テスト可能な最小単位、編集対象、非ゴールを確定する。前 Wave の統合と `VERIFY_ALL_OK` を完了条件とする。

## 3. 再開点と実装順序

2026-11-13 時点。Wave 1 は統合済みで、Gaussian Blur の並列化・Java/Rust ベンチマーク・README の比較グラフも完了している。macOS でツールバーが「□」になる問題は `086c8a5` で修正した。アイコンは `Toolbar.java` の図形を参照して描画し、フォントに依存しない。明暗テーマ、1倍・2倍の描画、全14ツールの選択、macOS arm64 release ビルドと `VERIFY_ALL_OK` を確認した。実機ウィンドウのスクリーンショット比較は未実施。

Wave 2 の P4.4a・P4.4b・P4.4c、および独立して進めた P4.9b を実装した。次は **P4.5c LUT のモデル・反転・保存**。未実装の単位は、本書の計画だけでは完了扱いにしない。

進捗 (2026-11-13): Devin SWE-2 の P4.4a (`8cc4188`) を統合し、DeepSeek の独立レビューを受けた。通常の Gray8/Color256/Gray16/Gray32 スタックの6方式、開始・終了スライス、方式の保持、反転 LUT の警告、マクロ記録、出力画像のアクティブ化に対応する。レビューで見つかった indexed LUT の消失、数値欄の扱い、投影失敗時の設定保存、記録文字列、不正スタックのパニックを修正した。Java の `GenericDialog` を直接使う `UiProbe` で数値欄と記録を比較し、LUT は投影後の RGB 表示も照合する。Z Project 関連の差分・回帰16件と UI 11件を追加・検証した。

残る制限: RGB と真の C/Z/T 投影は P4.4d1、濃度校正による signed-16 Sum の補正は校正モデルの拡張時に扱う。overlay は未モデル化で投影されない。方式の保持はアプリ起動中のみ。空範囲の Median は Java が例外を記録して出力を作らないため、Rust ではエラーとして返す。

進捗 (2026-11-13): P4.4b (`70e7a4e`) のスライス追加・削除を統合した。独立レビューで見つかったしきい値の保持と RGB 表示範囲の差を修正し、操作後のしきい値解除と RGB の 0..255 への復帰を Java と比較した。ROI とグレースケールの表示範囲は保持する。Alt による前方挿入、現在位置、ラベル、画素型、Undo と記録も検証対象。Z Project のメニューと差分テストを残して統合する。

P4.4b の制限: C/T 軸の編集は P4.4d2 まで明示的に拒否する。2枚から1枚に削除した場合、Java が StackWindow を保持するのに対し、Rust は枚数に基づきスライダーを隠す。ウィンドウ種別の保持と TYPE_CONVERSION の Undo は、既存モデルの制限として実機 UI 比較時にも確認する。

進捗 (2026-11-13): P4.4c の `Images to Stack` と `Stack to Images` を実装した。型・寸法の異なる画像の結合、4方式、補間、塗りつぶし、校正、ラベル、元画像の保持・閉鎖、ダイアログと記録に対応する。独立レビューの指摘を受け、2D 画像以外のウィンドウを閉じる不具合、1ピクセル幅・高さの補間、最大面積画像の寸法表示、重複タイトル、Delete Slice 後のラベル継承、記録の空白、同寸法画像を結合した後の方式設定を修正した。1ピクセル画像の補間は共有の resize 処理を直し、`Size...` の平均化あり・なしも Java と比較する。変換・リサイズの差分31件と UI 21件で確認する。再レビューではコピー結合時の8-bit表示範囲とLUT、RGB表示範囲、同寸法結合後のBicubic設定、複数スタック時のConcatenate案内も修正した。Delete Sliceで1枚になった際の `Slice_Label` プロパティは `slice_label_property` で保持し、結合で作った1枚スタックのラベルと区別する。

P4.4c の制限: hyperstack・composite の変換は明示的に拒否し、P4.4d2 で扱う。`Info`、overlay、`FileInfo` と Gray16/Gray32 の LUT 継承は未モデル化。Delete Sliceから結合する `Slice_Label` の経路と、その直列化は対応する。Stack to Imagesの単一画像エラーは、Javaの内部ID・型・寸法を含む画像記述をタイトルだけに短縮して表示する差がある。設定の保持はアプリ起動中のみ。

進捗 (2026-11-13): P4.9b の `Analyze > Summarize` は Mean・SD・Min・Max、NaN、ラベル、桁数と TSV 保存に対応した。独立レビューを受け、実際の Java `Analyzer` と Results の TextPanel を使う `UiProbe.resultsLifecycle` で Measure・Set Measurements・Summarize の連続実行を照合する。計測時に末尾の集計4行を削除する規則、ラベル非表示、列の追加・削除、割当時点の NaN 設定、表示桁数の保持も検証する。列を再追加したときに古い値が復活する問題と、Rust プラグインが追加した列が表示されない問題を修正した。しきい値・C/Z/T の列は現在の画像に合わせて設定変更時に追加する。台帳の Vertical Slice 1 分類は変更しない。

P4.9b の制限: 空の表で Java が例外を出す経路は `CoreError::EmptyResults` とし、UI の項目を無効にする意図的な差がある。Session は Results ウィンドウが存在する経路をモデル化し、headless Java の表全体をリセットする経路は再現しない。名前付き ROI の計測ラベルは未対応。スライスの長いラベルの短縮規則と、画像名が `Max` の場合の粒子ごとの集計行削除は追加確認が必要。

| 残っている制限 | 対応する単位 | 依存関係 |
| --- | --- | --- |
| 16/32-bit の白画像で表示 LUT が反転しない | P4.5c | LUT のモデルと保存・表示経路をそろえる。 |
| 複数領域や穴を持つ選択領域を表現できない | P4.3b | P4.10a の複合 ROI 交換より先に完了する。 |
| Size でハイパースタックの深さを変えられない | P4.4d3 | 通常スタック編集と C/Z/T 配列の検証後に行う。 |
| Java 版と実機上の外観を厳密に比較していない | Final | macOS 上でツールバー、メニュー、各ダイアログ、画像ウィンドウを並べて確認する。 |

基本の実装順序は Wave 2 → Wave 3 → Wave 4 → Wave 5 → Wave 6 とする。2026-11-13 の並列実行指示により、依存しない単位は独立 worktree で先行できる。P4.3b は Wave 2 統合後から着手でき、Wave 5 の前提となる。各 Wave の途中でも、表の小単位ごとに検証とコミットを終える。

## 4. 各単位でそろえる検証

- 画素型、画像寸法、スライス順序、現在の C/Z/T、ROI、校正、表示範囲、LUT、Undo、画像の変更状態を、そのコマンドが変更する範囲で Java と比較する。
- フィルタは通常画像、ROI、境界、定数画像、1ピクセル幅・高さ、スタックを試す。float を受け付ける演算では NaN、無限大、負値の扱いも Java で観測する。
- ダイアログは初期値、前回値、OK、Cancel、不正値、マクロ記録を検証する。未対応の設定を選択できる状態で公開しない。
- 新規画像や複数ウィンドウを作る操作は、入力画像の保持・閉鎖、タイトル、出力画像のアクティブ化も確認する。
- Java のエラーと Rust のエラー、失敗・Cancel 後の画像とウィンドウの状態を比較する。実行してから未対応と判明する経路を避ける。
- 大きな一時バッファを導入する操作は、代表サイズで処理時間とピーク RSS を記録する。画素一致の検証を通してから最適化する。RSS はプロセス全体の値として扱う。

差分テストは `crates/ij-session/tests/oracle_parity.rs`、UI は `crates/ij-app/tests/` に追加する。Java ホストの交換は `crates/ij-bridge/tests/` とホストの fixture、ファイル形式は `crates/ij-io/tests/` でも検証する。既存のオラクル・ホストのプロセス制御とベンチマークを再利用する。

## 5. Wave 2: スタックとカラー

| 単位 | 実装範囲 | 最初に読む Java | 先に追加する差分・UI 検証 |
| --- | --- | --- | --- |
| P4.4a | 通常のグレースケールスタックの `Z Project...`。Average、Max、Min、Sum、Standard Deviation、Median の6方式と開始・終了スライス。 | `ij/plugin/ZProjector.java` | 8/16/32-bit、各方式の出力型・丸め、部分範囲、1枚、定数、NaN、ROI、校正、入力画素が変わらないこと。ダイアログと記録。 |
| P4.4b | 通常スタックの `Add Slice`、`Delete Slice`。 | `ij/plugin/StackEditor.java` | 挿入位置と Alt、削除後の現在位置、1枚からの追加、最後の1枚の削除エラー、ラベル、全画素型、Undo と変更状態。 |
| P4.4c | `Images to Stack`、`Stack to Images`。 | `ij/plugin/ImagesToStack.java`, `ij/plugin/StackEditor.java` | 並び順、タイトル、ラベル、型・寸法違い、元ウィンドウの扱い、校正、Cancel。結合後の分解を Java とそれぞれ比較する。 |
| P4.4d1 | RGB・C/Z/T の Z Project。 | `ij/plugin/ZProjector.java` | チャンネル最優先のスライス配置、Z の投影範囲、現在フレームと全フレーム、RGB の処理、校正、チャンネル LUT。 |
| P4.4d2 | ハイパースタックのチャンネル・スライス・フレーム追加と削除。 | `ij/plugin/StackEditor.java` | C/Z/T の並び順、追加位置、各軸の最後の要素の扱い、ラベル、現在位置、LUT と校正。 |
| P4.4d3 | `Size...` のハイパースタック深さ変更。 | `ij/plugin/Resizer.java`, `ij/plugin/Scaler.java` | 変更対象の軸、深さ方向の補間と境界、C/T の保持、校正、現在位置、Cancel と記録。 |
| P4.5a | RGB と複数チャンネル画像の `Split Channels`。 | `ij/plugin/ChannelSplitter.java`, `ij/plugin/HyperStackReducer.java` | 出力画素・型、チャンネル名、Z/T、校正、現在位置、元画像を閉じる規則と Alt。単一チャンネルのエラー。 |
| P4.5b | `Merge Channels...` の RGB・composite 出力。 | `ij/plugin/RGBStackMerge.java`, `ij/CompositeImage.java` | 最大7チャンネル、未指定チャンネル、寸法・型・スタック長の違い、Keep source images、Ignore source LUTs、表示と記録。composite の表示状態を先にモデル化する。 |
| P4.5c | `Lookup Tables > Invert LUT` と `Color > Invert LUTs`。16/32-bit 白画像の表示反転も解消する。 | `ij/plugin/LutLoader.java`, `ij/plugin/SimpleCommands.java`, `ij/gui/NewImage.java`, `ij/process/ImageProcessor.java` | 画素が変わらず表示だけ反転すること、全画素型、単一・全チャンネル、しきい値、保存・再読込。New Image の White を再検証する。 |

Wave 2 内は P4.4a → P4.4b → P4.4c → P4.5c → P4.5a → P4.5b → P4.4d1 → P4.4d2 → P4.4d3 の順に進める。カラー表示と LUT のモデルを、分離・結合と高度な投影の前にそろえる。

主な編集先は `ij-core` のスタック・投影・表示モデル、`ij-session` の画像管理とコマンド、`ij-app` のスタック・カラーダイアログ。新しいドメイン処理は用途別モジュールに置く。C/Z/T の対応前に、通常スタックの実装で未対応次元を黙って平坦化しない。

## 6. P4.3b: 複合選択領域

`ij/gui/ShapeRoi.java`、`ij/plugin/filter/ThresholdToSelection.java`、`ij/plugin/Selection.java` を読む。複数輪郭と穴を保持する ROI 表現を設計し、ピクセル包含判定と境界座標を Java と比較する。輪郭の向きや接触点の規則は Java の出力で決める。

1. 複数領域、穴、斜め接触、画像境界に触れるマスクの `Create Selection` を差分テストにする。
2. ROI の包含判定、移動、マスク生成、画像外への Crop、Clear Outside、計測を順に対応させる。既存 ROI 型の全分岐と直列化の呼出元を確認する。
3. ROI Manager と表示を接続し、生成した ROI を再びマスクへ戻したときの画素を Java と比較する。

主な編集先は `ij-core` の ROI モデル・幾何・計測、`ij-session`、`ij-app` の ROI 描画と編集。Java ホストとの交換は P4.10a で扱う。

## 7. Wave 3: コントラスト、極大点、背景除去

| 単位 | 実装範囲 | Java の参照元と検証の要点 |
| --- | --- | --- |
| P4.6a | `Enhance Contrast...` の表示範囲調整と Normalize。 | `ij/plugin/ContrastEnhancer.java`。Saturated pixels、全画像と ROI、型別の画素変更、通常・スタックヒストグラム、既定値、Cancel を比較する。既存の `ij-core/src/contrast.rs` と表示ヒストグラムを確認する。 |
| P4.6b | 同コマンドの Equalize histogram。 | 同ファイルと各 Processor。ヒストグラムの積算順序、丸め、型、スタックの処理単位、Normalize との選択規則を照合する。 |
| P4.6c | `Find Maxima...` の極大点検出と点・数値出力。 | `ij/plugin/filter/MaximumFinder.java`。Prominence、Strict、平坦な山、同値の山、画像端、下限しきい値、Light background、NaN、結果の順序を比較する。 |
| P4.6d | 同コマンドの二値・領域分割出力と残りのダイアログ設定。 | 同ファイル。出力モードごとに独立した差分テストを追加し、分水嶺の境界と端の除外を画素単位で確認する。P4.6c の検出規則を再利用する。 |
| P4.7a | `Subtract Background...` の Rolling Ball、グレースケール。 | `ij/plugin/filter/BackgroundSubtracter.java`。半径、Light background、Create background、Disable smoothing、縮小率の境界、8/16/32-bit、ROI、スタック、NaN を比較する。 |
| P4.7b | Sliding paraboloid と RGB の Separate colors を含む残りの設定。 | 同ファイルと `ij/process/ColorProcessor.java`。色の分離規則、背景だけの生成、補正の順序、端の扱いを比較する。P4.7a とは別に検証する。 |

主な編集先は `ij-core` の演算モジュール、`ij-session`、`ij-app` の各ダイアログ。Find Maxima の検出と領域分割、Rolling Ball と Sliding paraboloid を一つの大きな変更にまとめない。

## 8. Wave 4: 周波数と計測

| 単位 | 実装範囲 | Java の参照元と検証の要点 |
| --- | --- | --- |
| P4.8a | FFT のデータモデルと純粋な変換処理。 | `ij/plugin/FFT.java`, `ij/process/FHT.java`。パディング、float の演算順序、元寸法と校正、変換前後の画素型を観測する。既存ライブラリは Java 差分を満たせるか試してから採用を決める。 |
| P4.8b | `FFT`、`Inverse FFT` の画面経路。 | 同ファイル。スペクトル表示と変換係数を別々に保持し、表示画像から逆変換できること、マスク編集、元寸法への復元、変換データのない画像のエラーを比較する。Java の逆変換結果を期待値にする。 |
| P4.9a | `Analyze > Calibrate...` の濃度校正。 | `ij/plugin/filter/Calibrator.java`, `ij/measure/Calibration.java`。空間校正とは別に関数・係数・単位を保持し、対応関数ごとに計測、Histogram、Plot Profile、TIFF 往復を検証する。 |
| P4.9b | Results の `Summarize`。 | `ij/plugin/filter/Analyzer.java` の `summarize`、`ij/measure/ResultsTable.java`。Mean、SD、Min、Max、0/1/複数行、NaN、動的な列、Label、連続実行、表示桁、保存を比較する。 |

主な編集先は `ij-core` の周波数・校正・Results モデル、`ij-io` のメタデータ保持、`ij-session`、`ij-app`。FFT の係数を画像のタイトルだけで識別しない。濃度校正は各計測の既存コードを確認してから接続する。

## 9. Wave 5: Java プラグインとマクロ

| 単位 | 実装範囲 | Java の参照元と検証の要点 |
| --- | --- | --- |
| P4.10a | 非矩形 ROI と indexed color の `PlugInFilter` 交換。 | `ij/plugin/filter/PlugInFilterRunner.java`, `ij/gui/ShapeRoi.java`, 各 Processor、既存 `tools/plugin-host/`。P4.3b を前提に、マスク、ROI の復元、LUT、SUPPORTS_MASKING、エラー時に Rust 側を変更しないことを fixture で検証する。 |
| P4.10b | ハイパースタックの交換。 | 同 Runner と `ij/ImagePlus.java`, `ij/ImageStack.java`。C/Z/T、現在位置、スライスラベル、チャンネル LUT と校正、DOES_STACKS の規則を参照ホストと比較する。 |
| P4.11a | `.ijm` の実行ホスト。既存の Java マクロ実行系を使う。 | `ij/plugin/Macro_Runner.java`, `ij/macro/Interpreter.java`, `ij/macro/Functions.java`。まず単一画像の実行・結果回収・エラー・中止・終了処理を検証する。実行入口は `ij-bridge` に置き、プロセス制御を再利用する。 |
| P4.11b | 複数画像・Results を使うマクロと実行画面。 | 同ファイル。画像選択、新規画像、Results、引数、ログ、複数ウィンドウの回収、途中失敗時の状態を比較する。回収対象のモデルを実装前に確定する。 |
| P4.11c | `.ijm` の開く・編集・保存・実行・中止。 | `ij/plugin/frame/Editor.java`。未保存の編集、ファイル名と文字コード、保存失敗、実行中の編集・終了、記録したマクロの再実行を検証する。 |

主な編集先は `ij-bridge`、`tools/plugin-host/`、`ij-session`、`ij-app`。Java 側の参照ファイルは編集しない。Rust でマクロ言語全体を再実装すること、任意の Fiji プラグインや複雑な独自 AWT UI の互換化は、この Wave の範囲に含めない。互換範囲は fixture と実行結果で示す。

## 10. Wave 6: 画像入出力と最終確認

| 単位 | 実装範囲 | Java の参照元と検証の要点 |
| --- | --- | --- |
| P4.12a | BMP の読み書き。 | `ij/plugin/BMP_Reader.java`, `ij/plugin/BMP_Writer.java`。既存 `image` 依存の利用を検討し、行の向き、パディング、パレット、RGB、Java の保存変換を比較する。 |
| P4.12b | GIF の読み書きと複数フレーム。 | `ij/plugin/GIF_Reader.java`, `ij/plugin/GifWriter.java`。パレット、透明色、フレーム順、画像型変換、保存対象スライスを Java と比較する。 |
| P4.12c | Raw の読み書き。 | `ij/plugin/Raw.java`, `ij/io/ImportDialog.java`, `ij/io/FileOpener.java`, `ij/io/FileSaver.java`。型、バイト順、ヘッダー、オフセット、スライス間隔、不足・余分なデータ、不正寸法を検証する。 |
| P4.12d | Text Image の読み書き。 | `ij/plugin/TextReader.java`, `ij/plugin/TextWriter.java`。小数、負数、区切り、行長の違い、NaN、表示桁、型の決定を Java と比較する。 |
| P4.12e | 連番画像の読み込み・保存。 | `ij/plugin/FolderOpener.java`, `ij/plugin/StackWriter.java`。並び順、開始・枚数・間隔、名前の絞り込み、型・寸法不一致、ラベル、途中エラーと Cancel を検証する。 |

コーデックとファイル操作は `ij-io`、ファイル選択と進捗・中止は `ij-app`、画像管理は `ij-session` に置く。既存ライブラリを使う場合も、Java と異なる既定の変換をそのまま公開しない。

Final では以下を一つずつ完了する。

1. 連番読込 → Z Project → 背景除去 → しきい値 → 複合選択 → 計測 → Summarize → 保存・再読込を Java と比較する。
2. チャンネル分離・結合、FFT 往復、Java フィルタ、記録した `.ijm` の再実行を、それぞれ入出力とメタデータまで確認する。
3. macOS のビルドから起動し、ImageJ 1.54u とツールバー、メニュー、画像ウィンドウ、主要ダイアログを実機で比較する。テーマ、Retina、キーボード操作も確認する。
4. 英日 README とコマンド台帳の対応状況をそろえ、残る制限を記す。未検証の全 ImageJ/Fiji 互換性を宣言しない。
5. 統合した `main` で `./tools/verify.sh` の `VERIFY_ALL_OK` を確認する。計算量の大きい操作は処理時間と RSS の測定条件・生データも残す。

## 11. 担当と統合

前回の実装で使った Devin SWE-2 と Mistral Vibe `ds-flash` による相互レビューを継続する。実際に着手するときに登録名と利用可否を確認する。実装担当とレビュー担当は単位ごとに入れ替え、レビュー指摘への対応後に再検証する。

最初の P4.4a は実装、Java 差分確認、UI 配線、レビューの順で一つの変更として進める。その後に独立した演算を並列化する場合は worktree を分ける。`AppAction`、メニュー表、Session のコマンド列挙、共有の差分テストは統合担当が順に反映し、同じファイルを複数担当が共有 checkout で同時に編集しない。

各単位の報告には、Java の根拠、変更ファイル、失敗した先行テスト、検証ログ、コミット、残る制限を含める。新しい設定や結果モデルが必要になった場合は、そのモデルと呼出元の変更範囲を先に確定する。
