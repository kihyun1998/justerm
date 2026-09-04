# justerm

VT 바이트 스트림을 터미널 화면 상태(그리드 + 스크롤백)로 짜 넣는 **순수 터미널 엔진** (Rust).
렌더러도 emulator 도 아니다 — 화면을 *그리지 않고*, 화면 *상태와 변경분(damage)* 을 만들어 노출한다.

`justerm` 은 크레이트가 아니라 **패밀리 umbrella 이름**이다: 코어 `justerm-core` + wasm 디코더
`justerm-wasm-decode` + 웹 위젯 `justerm-web` + 자체 렌더러 `justerm-renderer`(`justerm-facade` 는
옛이름 묘비). 엔진 라이브러리에서 first-party 풀스택으로 피벗했고(ADR-0012→0018), 그 피벗에도
***core 경계는 불변***이다 — core 는 여전히 안 그린다. 첫 소비처는 **PenTerm**(Tauri 터미널 앱)이지만
`justerm-core` 는 penterm 전용이 아닌 재사용 가능한 독립 크레이트다.

## 경계 invariant (이게 정체성)

justerm 이 **하는 것**: vte 로 VT 스트림 파싱 → 셀 그리드 + 스크롤백 + 커서 + selection 상태 보유 →
*뷰포트 스냅샷 + damage(줄+열범위) + scroll op* 를 노출. text 추출(복사) 제공.

justerm 이 **하지 않는 것** (의존성으로 끌어들이지도 말 것):
- **I/O 없음** — PTY/SSH/소켓 안 읽음. 호출자가 바이트를 `feed()` 로 넣는다.
- **IPC 없음** — Tauri/채널/전송 안 함. 바이너리 *포맷*은 제공하되 *전송*은 소비처 몫.
- **렌더링 없음** — core 는 GPU/캔버스/그리기 안 함. 패밀리의 `justerm-renderer` 가 그린다(별도
  크레이트 — core 는 여전히 화면 상태·damage 만 노출한다).
- **theme 무지(theme-agnostic)** — 색을 *참조*(Default / Indexed(u8) / Rgb)로만 저장. 팔레트→실제
  색 해석은 *소비처/렌더러* 가 frozen 스킴으로. justerm 은 hex 색을 영영 모른다.

→ 결과: PTY 도 Tauri 도 GPU 도 없이 **독립 테스트 가능**(vttest + 단위테스트).

**core 냐 소비처냐 (라우팅 규칙, ADR-0017)**: 기능의 *메커니즘*은 ① VT-파싱이거나 ② 올바르려면 *버퍼
전체*(전 셀·스크롤백·좌표·wrap·wide-char)가 필요하면 **core**(frame 모드 소비처는 뷰포트만 쥐어
물리적으로 못 함) — 단 *정책*(query·regex·palette)은 소비처가 주입해 core 는 policy/theme-agnostic
유지(**메커니즘 core, 정책 소비처**). 그 외(색해석·hover·픽셀→셀·debounce·스크롤바·클립보드·전송)는
소비처. **우회 금지**: 다른 층의 결함을 소비처에서 덮지 말고 멈춰서 사용자에게 말한다.

## 어디를 먼저 읽나

- **`docs/map/`** (허브 `docs/map/README.md`) — 착수 전 배선도. *"이걸 건드리면 뭐가 같이 움직이나"*
  (수평)와 *"이 코드는 어떤 결정에서 나왔나"*(수직)에 답하는 링크 그래프. ADR·architecture.md 는
  **사건**으로 색인돼 있어 둘 다 못 답한다. 유지 규율은 그 README 의 § Maintenance.
- **`docs/architecture.md`** — 셀·damage·뷰포트/스크롤·cadence·selection·직렬화·엔진 API 의
  authoritative 계약.
- **`docs/adr/`** — 결정과 그 근거. **여기에 ADR 목록도 status 도 적지 않는다**: 파일명이 이미 한 줄
  요약이고 각 파일 머리의 `Status:` 줄이 authoritative 이다. 복사본은 게이트가 없어 조용히 낡는다 —
  이 문단이 실제로 그렇게 틀렸었다.
- **`CONTEXT.md`** — glossary.
- 큰 그림·빌드플랜은 GitHub **Epic #1**(엔진) + 슬라이스 #2–#12, 이후 **#103**(web)·**#258**(renderer).
- 설계 출처(역사): penterm 의 `.scratch/rust-terminal-engine/PRD.md` — 이 계약이 grill 로 확정된
  2026-06-16 원본 기록. 근거를 더 파고 싶을 때만.

## 기술 스택

Rust (edition 2024). 핵심 의존성은 **`vte`** (Paul-Williams ANSI 파서 — *진짜 어려운 파싱*만 안정
크레이트에 위임하고, 그 위 grid/스크롤백/selection 은 자작). `alacritty_terminal` 은 **의존 안 함**
(API 불안정) — 모델 설계의 *참고*일 뿐. 근거는 ADR-0001.

## 개발 명령어

```bash
cargo test --workspace   # 코어 + justerm-wasm-decode 바인딩까지 (--workspace 필수)
cargo bench              # throughput 마이크로벤치(추세 기록)
```

루트는 가상 매니페스트(`[package]` 없음)라 `--workspace` 로 멤버를 명시해야 하고, 그마저도
`fuzz`·`justerm-facade`·`justerm-renderer`·wasm32-전용 테스트(`justerm-wasm-decode/tests/web.rs`)는
***빌드조차 안 한다***. **게이트 목록도 개수도 여기 적지 않는다** — 권위 있는 것은
`.github/workflows/test.yml` 의 `run:` 줄이고, 그게 잡별로 무엇이 돌고 무엇이 안 도는지 말한다.

## 핵심 규칙

- **주석**: 영어. **CONTEXT.md / docs/adr/ / docs/map/**: 영어(LLM 토큰 효율 — `docs/map/` 은 *모든*
  착수 시점에 에이전트가 읽는다). 그 외 사람이 읽는 문서·CLAUDE.md: 한국어.
- **네이밍**: Rust 관용(snake_case 함수/모듈, CamelCase 타입).
- **커밋 메시지**: 관련 GitHub 이슈 번호 참조 (`feat: ... (#12)`). `Co-Authored-By` trailer 금지.
- **컴플라이언스는 누적**: VT 정합성(8.6K SLoC급 long tail)은 한 방에 못 짠다 — 공통 90% 부터,
  dogfood 가 깨는 케이스를 만나며 tail 을 키운다. *뼈대(계약/경계)는 처음부터 옳게*.
- **디렉터리 경계는 seam 이 물리적으로 표현된 자리다.** 파일을 틀린 곳에 쓰면 seam 이 깨지는데 에러도
  실패 테스트도 경고도 안 난다 — 쓰기 *전에* **ADR-0030**(구체 경로).

## Agent skills

**작업 규율은 `thegraph` 스킬이다** — substantive 변경(core·wasm·web·renderer)은 착수 시 `/thegraph`.
노드 그래프도 불변식도 방법론도 **스킬이 소유하므로 여기 옮겨 적지 않는다**. repo 가 대는 데이터는
`docs/agents/thegraph.md` 한 장뿐 — 이 repo 가 어떤 외부 소스를 상대로 지어졌는가(무엇을 알려주고,
어떻게 읽고, 어느 것이 *binding* 인가). 레퍼런스는 `../.refs/` 의 SHA 핀된 로컬 체크아웃을 `rg` 로
읽고(`WebFetch`·통파일 `gh api` 금지 — 메서드 본문이 조용히 잘린다), 이미 확인된 사실은
`docs/agents/reference-facts.md` 에 `file:line`+SHA 로 쌓여 있으니 백지에서 시작하지 말 것.

- **teach 코스** — `/teach` 는 **먼저 `teach/README.md`** 를 읽는다(워크스페이스 위치·규칙·진행상황).
- **Issue tracker** — GitHub issues via `gh`. `docs/agents/issue-tracker.md`.
- **Triage labels** — 각 triage 역할의 라벨 = 그 이름. `docs/agents/triage-labels.md`.
- **Domain docs** — single-context, `CONTEXT.md` + `docs/adr/`. `docs/agents/domain.md`.
- **Releasing** — `vX.Y.Z` 태그 push 가 crates.io + npm 을 *자동* 발행한다(수동 `cargo publish`/
  `npm publish` 금지, 충돌남). 규약·downstream 절차는 `docs/agents/release.md`.
- **Supply-chain** — CI `supply-chain` 게이트는 **just-shield**(first-party, 형제 repo
  `../just-shield`)로 워크플로를 `scan --strict`. 결정은 ADR-0006, 운영은 `docs/agents/supply-chain.md`.
- **`docs/agents/theflow.md` 는 은퇴한 규율의 파일인데 지우면 안 된다** — 라우팅 대상이 아니라 *인용
  대상*이다(인바운드 약 40건, 그중 일부는 docs.rs 로 나가는 doc-comment 와 ADR·`docs/map/` 노트).
  거기가 **여전히 유일한 홈**인 것: § "Architecture prior art", 소비처 매니페스트 두 곳(**penterm 의
  Rust dep 는 `src-tauri/Cargo.toml` 아래라 top-level grep 은 "소비처 없음" 이라고 거짓 보고한다**),
  증명 수단 표. 단 그 안의 **핀 표는 4-tree 구판**이니 보지 말 것.
