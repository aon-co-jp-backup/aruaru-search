
## 2026-11-04 依存更新 / Dependency update
- reqwest 0.12→0.13（TLS機能名は`rustls-tls`→`rustls`）。ビルド・テスト44件成功。`.deps/`配下の他リポジトリ由来の遅れ（bincode/russh/rand等）と、修正版の無いrsa（RUSTSEC-2023-0071）は各リポジトリ側の対応待ち。
- reqwest 0.12→0.13 (TLS feature renamed `rustls-tls`→`rustls`). Build and 44 tests pass. Gaps inside `.deps/` (bincode, russh, rand...) and rsa (RUSTSEC-2023-0071, no fix) belong to the upstream repos.
