<p align="center">
  <img src="src/assets/logo.svg" width="128" height="128" alt="Clawy Logo" />
</p>

<h1 align="center">Clawy</h1>

<p align="center">
  <strong>OpenClaw 向けの軽量な Tauri デスクトップワークスペース</strong>
</p>

<p align="center">
  <a href="#機能">機能</a> •
  <a href="#なぜclawyなのか">なぜClawyなのか</a> •
  <a href="#はじめに">はじめに</a> •
  <a href="#アーキテクチャ">アーキテクチャ</a> •
  <a href="#開発">開発</a>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux-blue" alt="Platform" />
  <img src="https://img.shields.io/badge/tauri-2+-24C8DB?logo=tauri" alt="Tauri" />
  <img src="https://img.shields.io/badge/react-19-61DAFB?logo=react" alt="React" />
  <img src="https://img.shields.io/github/downloads/edwardZhang/Clawy/total?color=%23027DEB" alt="Downloads" />
  <img src="https://img.shields.io/badge/license-MIT-green" alt="License" />
</p>

<p align="center">
  <a href="README.md">English</a> | <a href="README.zh-CN.md">简体中文</a> | 日本語
</p>

---

## 概要

**Clawy** は、OpenClaw のために Tauri で再構築されたデスクトップアプリです。OpenClaw のランタイム、スキル、プロバイダー、チャネル、自動化機能を、より軽く、より速く、日常利用しやすいデスクトップ体験としてまとめています。

Clawy は「軽量なデスクトップシェル + 管理されたランタイム」という考え方を重視しています。

- 配布パッケージをより軽くできる
- 起動が速く、操作感が軽い
- 必要なランタイムをアプリ自身で検出・準備できる
- OpenClaw をデスクトップシェルから分離して更新できる

## なぜClawyなのか

| 課題 | Clawy のアプローチ |
|---|---|
| デスクトップパッケージが肥大化しやすい | Tauri ベースの軽量シェルで余分なオーバーヘッドを削減 |
| コールドスタートが重い | ネイティブウィンドウ + WebView 構成で起動応答を改善 |
| ユーザーに手動セットアップを要求したくない | 起動時に Node.js を検出し、必要なら自動準備 |
| OpenClaw を毎回デスクトップ更新に縛りたくない | OpenClaw ランタイムを独立して準備・更新可能にする |
| OpenClaw は CLI 中心になりやすい | チャット、スキル、チャネル、スケジュールをデスクトップ UI で管理 |

## 機能

### 軽量なパッケージと高速起動

Clawy は **Tauri** ベースで構築されており、デスクトップフレームワーク自体の負担を抑えながら、より素早い起動体験を目指しています。

### Node.js の自動検出と準備

Clawy は起動後に必要な Node.js 環境があるかを確認し、不足している場合はアプリ側で自動準備できる設計です。

### OpenClaw サービスの自動準備

OpenClaw サービスもアプリが管理するランタイムとして扱い、ユーザーが手動で複雑なセットアップを行わなくても使い始められるようにします。

### OpenClaw の独立更新

OpenClaw ランタイムはデスクトップシェルと分離して管理できるため、サービス更新をより柔軟に進められます。

### OpenClaw ワークフロー全体をデスクトップで管理

- チャットと会話履歴
- AI プロバイダー設定
- スキルの参照、導入、管理
- チャネル設定
- 定期実行タスク
- ランタイム状態、ログ、診断

## はじめに

### 動作要件

- macOS 11+、Windows 10+、または Linux
- 4 GB RAM 以上
- ランタイム準備と更新のためのネットワーク接続

### インストール

[Releases](https://github.com/edwardZhang/Clawy/releases) から最新版を取得してください。

### 初回起動

初回起動時、Clawy はセットアップとランタイム確認を順に案内します。

1. 言語と基本設定を選択
2. 必要なランタイムの有無を確認
3. Node.js がなければ自動準備
4. OpenClaw サービスランタイムを準備または取得
5. AI プロバイダーを設定
6. メイン画面へ進む

### プロキシ

Clawy には、デスクトップランタイム、OpenClaw Gateway、外部チャネルがローカルプロキシ経由で通信するための設定が用意されています。

設定可能な項目：

- Proxy Server
- Bypass Rules
- HTTP Proxy
- HTTPS Proxy
- ALL_PROXY / SOCKS

保存後はデスクトップ側のプロキシ設定が再適用され、Gateway も自動的に再起動されます。

## アーキテクチャ

Clawy は次のような構成を想定しています。

```text
Clawy Desktop Shell (Tauri)
  -> ウィンドウ管理、トレイ、更新、システム統合
  -> ランタイム確認と初期化
  -> 設定、ログ、診断

Managed Runtime Layer
  -> Node.js の検出 / ダウンロード
  -> OpenClaw ランタイムの準備
  -> ランタイムのバージョン管理

OpenClaw Runtime
  -> gateway
  -> skills / plugins
  -> channels
  -> provider / agent orchestration
```

## 開発

### 前提

- Node.js 22+
- pnpm 10+
- Tauri ビルド用の Rust ツールチェーン

### よく使うコマンド

```bash
pnpm run init
pnpm dev
pnpm run lint
pnpm run typecheck
pnpm test
pnpm run build:tauri
```

### ソースから起動

```bash
git clone https://github.com/edwardZhang/Clawy.git
cd Clawy
pnpm run init
pnpm dev
```

## 謝辞

Clawy は次の優れたオープンソースプロジェクトの上に成り立っています。

- [OpenClaw](https://github.com/OpenClaw)
- [Tauri](https://tauri.app/)
- [React](https://react.dev/)
- [Zustand](https://github.com/pmndrs/zustand)

Clawy は ClawX を参考にしつつ Tauri で再構築したプロジェクトでもあります。元の方向性と基盤を作った ClawX の開発者のみなさんに感謝します。

## ライセンス

Clawy は [MIT License](LICENSE) の下で公開されています。
