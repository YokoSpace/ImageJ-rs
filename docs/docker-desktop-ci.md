# Docker デスクトップ CI ランオブック

## Issue を担当したら

1. Issue の受け入れ条件を確認し、対応する Java 実装を読む。修正前に、その挙動を観測するテストを書く。条件が複数あれば、各条件について RED と GREEN を確認する。
2. 下のコマンドでそのテストだけを実行する。想定した assertion で失敗したことをログで確認してから実装する。ビルド失敗やテスト名の誤りは RED に数えない。
3. 同じテストを再実行する。通ったら素の Compose 構成で最終ゲートを通し、終了コード、マーカー、証跡を報告する。失敗したら表示された原因行を読み、該当箇所を直して続ける。停止条件はロードマップ §0.2 に従う。

以下は既存テストで手順を試す例。担当 Issue では3つの引数をその回帰テストに置き換える。モジュール内のテスト名には `module::name` を使う。

```sh
export COMPOSE_PROJECT_NAME=imagej-issue-123 DESKTOP_PORT=13002
export COMPOSE_FILE=compose.yaml:docker/compose.dev.yaml
./docker/ci up
./docker/ci test ij-app new_image_ui_test file_new_menu_wires_new_image
# crates/ を編集したら同じ test を再実行
unset COMPOSE_FILE
./docker/ci check ij-app new_image_ui_test file_new_menu_wires_new_image
```

開発用の構成はホストの `crates/` を読み取り専用でマウントする。このツリー内の編集は即時反映される。ツールや依存設定を編集したら `./docker/ci up` で再ビルドする。

すべての受け入れ条件を確認し、最終コマンドが終了コード 0 で `TEST_OK <名前>`、`VERIFY_ALL_OK`、`DESKTOP_SMOKE_OK`、`evidence: <パス>` を出したら完了。ネイティブ操作に固有の Issue は、その操作と期待結果も実際のウィンドウで確認する。標準 smoke の検証範囲は起動から Ctrl+Q による正常終了まで。

## 基本コマンド

すべてリポジトリ直下の `./docker/ci` に集約されています。

```sh
./docker/ci test ij-app new_image_ui_test  # ターゲット全体を実行
./docker/ci check      # up + verify + smoke + evidence を一括実行
./docker/ci up         # イメージをビルドしデスクトップを起動(ヘルスチェック完了まで待つ)
./docker/ci verify     # コンテナ内で ./tools/verify.sh(最後に VERIFY_ALL_OK が必須)
./docker/ci smoke      # docker/desktop-smoke.sh(ウィンドウ出現→Ctrl+Q でクリーン終了)
./docker/ci evidence   # verification/evidence/latest を desktop-evidence/run.XXXX/latest にコピー
./docker/ci down       # docker compose down
```

ブラウザでデスクトップを見る場合は `https://localhost:3001/`(自己署名証明書のため警告が出ます)。ポートはループバックのみで、`DESKTOP_PORT` で変更できます。複数環境の並行実行は `COMPOSE_PROJECT_NAME` と `DESKTOP_PORT` を両方変えてください。

## ログと証跡

`./docker/ci` はビルド・テスト・証跡取得などの実行ごとに `desktop-evidence/run.XXXXXXXX/` を作り、各ステップの全出力を `<ステップ名>.log` に保存します。画面には `RUN`/`OK`/`FAIL` と失敗時は原因候補を先頭から最大3行（各240バイトまで）と、末尾16行（最大3072バイト）だけが出ます。`docker compose logs` の生ログはストリームせず、`check` 失敗時のみ `docker-logs.txt` に末尾 200 行が自動保存されます。`tail -f` で追いかけないでください。

診断するときは保存済みログを絞り込んで読みます:

```sh
rg -n -m 5 'error|FAILED|panicked' desktop-evidence/run.XXXXXXXX/verify.log | cut -c1-240
tail -c 3072 desktop-evidence/run.XXXXXXXX/verify.log | tail -n 16
```

`evidence` 成功後の `latest/` には `Results.tsv`、`ramp_saved.tif`、`recorded_macro.ijm`、`slice1_stdout.txt`、`parity_stdout.txt` が入ります。両方のマーカー(`VERIFY_ALL_OK` と `DESKTOP_SMOKE_OK`)と課題固有テストがそろうまで完了と報告しないでください。

## GUI 操作

```sh
./docker/ci open                  # ij-app をビルドしてデスクトップ上で起動(detached)
./docker/ci windows               # ij-app が持つ可視ウィンドウを ID\tタイトル で列挙
id=$(./docker/ci windows | awk -F '\t' '$2 == "ImageJ 1.54u" {print $1; exit}')
./docker/ci screenshot            # 操作前の位置を確認
./docker/ci click "$id" 320 40    # ウィンドウ相対座標(0以上の整数・範囲内のみ)でクリック
./docker/ci screenshot            # 操作後の表示を PNG で確認
./docker/ci key "$id" ctrl+q      # 操作が終わったらクリーンなウィンドウを閉じる
```

`windows` が出す ID は X11 のウィンドウ ID で、egui のウィジェットや画面絶対座標とは別物です。`click` の座標はウィンドウ内相対なので、先に `screenshot` を取って位置を確認してください。`key`/`click` は ij-app 所有のウィンドウ以外を拒否します。Xfce デスクトップ上で動くネイティブ X11 ウィンドウであり、macOS のネイティブメニュー動作はこの環境では検証できません。

## GitHub Actions と完了報告

Desktop CI は push、pull request、手動実行で起動する。`docker/tests/ci-errors.sh` でラッパーの異常系を先に試し、Linux で最終ゲートを実行する。Actions の `desktop-evidence` アーティファクトには成功時も失敗時もその時点までのログが入る。保存期間は7日間。

報告には Issue の受け入れ条件、RED/GREEN のテスト名、最終ゲートの終了コードとマーカー、証跡のパスを記す。停止条件に達した場合は、失敗したステップ、最初の原因行、試した修正を残す。
