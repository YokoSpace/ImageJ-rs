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
| P4.1b | `File > New > Image...`。`ij/plugin/NewImage.java`, `ij/gui/GenericDialog.java` | 名前、型、塗りつぶし、幅、高さ、スライス数、既定値、Cancel、マクロ記録が一致する。P4.1a の `VERIFY_ALL_OK` 後に着手する。 |
| P4.2a | `Image > Transform > Flip Horizontally/Vertically`, `Rotate 90 Degrees Right/Left`。`ij/plugin/Transformer.java`, `ij/process/*Processor.java` | 8/16/32-bit/RGB と ROI・スタックの Java 差分テストで画素、寸法、Undo が一致する。 |
| P4.2b | `Image > Transform > Rotate...` と `Image > Adjust > Size...`。`ij/plugin/Rotator.java`, `ij/plugin/Scaler.java`, 各 Processor | 補間、背景、キャンバス、寸法・校正、Cancel とマクロ引数を Java と比較する。P4.2a の `VERIFY_ALL_OK` 後に着手する。 |
| P4.3a | `Edit > Selection > Create Mask / Create Selection / Clear Outside`。`ij/plugin/Selection.java`, `ij/plugin/filter/Filler.java` | 二値・しきい値・非矩形 ROI の各規則と元画像への影響が一致する。対象が複数 ROI 型へ広がる場合は小単位に分ける。 |

P4.1 は Devin SWE-2、P4.2 は Mistral Vibe `ds-flash`、P4.3 は Antigravity で並行する。各担当は Java の根拠、失敗テスト、最終検証結果とコミットを提出する。ブランチ整理は統合担当が実施する。

## 2. 後続 Wave

| Wave | ID | 対象と順序 |
| --- | --- | --- |
| 2 | P4.4, P4.5 | `Image > Stacks` の Z Project、スライス追加・削除、画像とスタックの相互変換。その後 `Image > Color` のチャンネル分離・結合、LUT 反転。 |
| 3 | P4.6, P4.7 | `Enhance Contrast...` と `Find Maxima...`、続いて `Subtract Background...` の Rolling Ball。各ダイアログの既定値とスタック規則も照合する。 |
| 4 | P4.8, P4.9 | `Process > FFT / Inverse FFT` と `Analyze > Calibrate...`、Results の `Summarize`。周波数データと濃度校正メタデータの保持を別々に検証する。 |
| 5 | P4.10, P4.11 | Java `PlugInFilter` への非矩形 ROI・indexed color・hyperstack の受け渡し、その後 `.ijm` の実行・編集。既存 Java マクロを利用する経路も評価する。 |
| 6 | P4.12, Final | BMP/GIF/Raw/Text Image と連番画像 I/O。最後にワークフロー全体の Java 差分、README 更新、`VERIFY_ALL_OK` を確認する。 |

後続 Wave の着手前に各機能の Java 実装、既存 API、テスト可能な最小単位、編集対象、非ゴールを確定する。前 Wave の統合と `VERIFY_ALL_OK` を完了条件とする。
