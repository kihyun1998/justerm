# Resources

가장 신뢰도 높은 1차 출처부터. justerm 을 배우는 데 **repo 자체가 최상위 출처**다 (실제 배우는 대상이므로).

## Tier 0 — 이 repo (authoritative, 오프라인)
| 자료 | 위치 | 무엇 |
|------|------|------|
| **CLAUDE.md** | repo 루트 | 경계 invariant("이게 정체성"), 사고방식, 작업 flow, 게이트. 리뷰어의 성경. |
| **docs/architecture.md** | repo | 셀·damage·뷰포트/스크롤·cadence·selection·직렬화·엔진 API 의 authoritative 스펙 + "Hidden VT state". |
| **CONTEXT.md** | repo | 도메인 용어(ubiquitous language). |
| **docs/adr/0001–0018** | repo | 각 설계 결정의 근거 + 기각된 대안. "왜?"의 답. |
| **docs/agents/** | repo | issue-tracker·release·supply-chain·triage 운영. |
| **GitHub issues** | `gh issue list` | 진행 상태·slice·근거 토론. Epic #1 + slice #2–#12 가 빌드플랜. |

## Tier 1 — VT / 터미널 도메인 (외부, canonical)
| 자료 | URL | 무엇 | 신뢰도 |
|------|-----|------|--------|
| **A parser for ANSI-compatible terminals** (Paul Williams) | vt100.net/emu/dec_ansi_parser | justerm 이 쓰는 `vte` 크레이트가 구현한 **바로 그 상태머신**. VT 파싱의 canonical 다이어그램. | ★★★ (ADR-0001 이 이걸 근거로 vte 채택) |
| **XTerm Control Sequences** (invisible-island) | invisible-island.net/xterm/ctlseqs/ctlseqs.html | 이스케이프 시퀀스 사전(CSI/OSC/DEC 모드). | ★★★ 사실상 표준 |
| **vte 크레이트 docs** | docs.rs/vte | justerm 이 파싱을 위임한 크레이트의 실제 API. | ★★★ 실제 의존성 |

## Tier 2 — Prior art (설계 교차검증용, CLAUDE.md 가 지정)
| 자료 | 무엇 | 언제 |
|------|------|------|
| **alacritty_terminal** (github alacritty/alacritty) | 모델 설계 참고(damage `LineDamageBounds` 등). *의존은 안 함*. | 셀/damage 모델 "왜 이 grain?" 팔 때 |
| **xterm.js** (github xtermjs/xterm.js) | 버퍼/파서 층 + 소비처 동작(reflow·selection). | 메커니즘 대조 |
| **Mosh** (SSP) | ack-paced diff cadence 의 prior art. | cadence 팔 때 |

## Tier 3 — Rust 기초 (필요할 때만, 리뷰 수준)
| 자료 | URL | 무엇 |
|------|-----|------|
| **The Rust Book** | doc.rust-lang.org/book | 소유권·트레잇·enum. 리뷰에 걸리는 개념만 발췌. |
| (Rust by Example) | doc.rust-lang.org/rust-by-example | 짧은 실행 가능 예제. |

## 커뮤니티 (wisdom — 필요 시)
- 아직 미탐색. 터미널 엔진은 니치라 r/rust, alacritty/vte GitHub Discussions 정도가 후보.
  사용자가 원하면 고신뢰 커뮤니티를 탐색해 채운다.
