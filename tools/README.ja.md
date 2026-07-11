<!-- i18n: language-switcher -->
[English](README.md) | [日本語](README.ja.md)

# コネクタメタデータ

`connector.source.json` は各拡張機能の人間が編集可能な真の情報源です。
`connector.config.json` と `irodori.extension.json` は現在のネイティブABIおよびマーケットプレイスのレイアウトとの互換性を保つために生成されるパッケージング成果物です。

共通のコネクタメタデータジェネレーターは `irodori-table` コーディネーターリポジトリにあります。この拡張機能リポジトリは生成された成果物とローカルのREADMEヘルパーのみを保持します。

## コマンド

生成されたコネクタメタデータから英語のREADMEファイルを再生成します：

```sh
python3 tools/generate_readmes.py
```