# justerm

VT 바이트 스트림을 터미널 화면 상태(그리드 + 스크롤백)로 짜 넣는 **순수 터미널 엔진** (Rust).
렌더러도 emulator 도 아니다 — 화면을 *그리지 않고*, 화면 *상태와 변경분(damage)* 을 만들어 노출한다.

- **엔진 = `justerm-core`** (이 repo 의 코어 크레이트, 파싱+상태) / **렌더러 = `justerm-renderer`** (그리드를
  WebGL2 로 그림, first-party 패밀리 크레이트) — third-party `beamterm`(ADR-0002)을 자체 렌더러로 **대체 완료**
  (ADR-0018, Epic #258; justerm-web 스위치 #273 머지, 문서 플립 #274). → `-term` 패밀리. `justerm` 은 *패밀리 umbrella* 이름이다(코어
  `justerm-core` + wasm 디코더 `justerm-wasm-decode` + 웹 위젯 `justerm-web` + 자체 렌더러 `justerm-renderer`;
  `justerm-facade` 는 옛이름 묘비) — v0.6.0 에서 맨이름 `justerm` 을 개명(ADR-0010). justerm 은 '엔진
  라이브러리'에서 'first-party 풀스택'으로 피벗함(ADR-0012→0018) — 단 *core 경계는 불변*(core 는 여전히 안 그림).
- **첫 소비처 = PenTerm** (Tauri 터미널 앱). `justerm-core` 는 penterm 전용이 아니라 *재사용 가능한 독립
  크레이트*다.
- **상세 계약(구현 시 참조)**: **`docs/architecture.md`** — 셀·damage·뷰포트/스크롤·cadence·
  selection·직렬화·엔진 API 의 authoritative 스펙. 핵심 결정 근거는 `docs/adr/`(0001 vte·0002
  beamterm→0018 justerm-renderer·0019 셀 합성 모델 — 렌더러가 셀 하나를 bg/fg/잉크로 푸는
  *전역함수*, xterm 은 validator 아닌 설계 입력). 최근 4개는 *proposed*(작성됨, 미판정):
  **0020** 프레임 스냅샷에 실릴 자격(상태냐 사건이냐 / 소비처가 이미 쥐었나 / 뷰포트로 유계인가 —
  wire 그룹을 하나 더 얹기 전에 통과해야 하는 3규칙), **0021** 전역 WebGL2 컨텍스트 1개가 N 그리드를
  뷰포트로(`TerminalSurface`, 자원 3층 + 층 배정 규칙; #287), **0022** 셀 = 폰트 `█` 의 잉크 박스와
  거기서 파생되는 모든 기하(측정 방식은 beamterm 물림, 근거 미검증으로 등급 표시),
  **0023** 간격 설정의 단위는 CSS px(=`font_size` 와 같은 공간; 양 레퍼런스는 device px 라 한 폰트 서술이
  두 단위를 말함), **0024** decoration 은 *색 + 마크*이지 객체가 아님 → 투영/precedence 규칙 6개가
  거기서 파생(셀=등록순서, ruler=클래스 먼저; ADR-0019 가 out of scope 로 밀어낸 축).
  큰 그림·빌드플랜은 GitHub **Epic #1**(엔진, closed) + 슬라이스
  #2–#12, 이후 **#103**(web)·**#258**(renderer). *이 repo 안에서 전부 참조 가능* — penterm 안 봐도 됨.
- **설계 출처(역사)**: penterm 의 `.scratch/rust-terminal-engine/PRD.md` — 이 계약이 grill 로
  확정된 원본 기록(2026-06-16, prior-art 교차검증). 근거를 더 파고 싶을 때만 참조.

## 경계 invariant (이게 정체성)

justerm 이 **하는 것**: vte 로 VT 스트림 파싱 → 셀 그리드 + 스크롤백 + 커서 + selection 상태 보유 →
*뷰포트 스냅샷 + damage(줄+열범위) + scroll op* 를 노출. text 추출(복사) 제공.

justerm 이 **하지 않는 것** (의존성으로 끌어들이지도 말 것):
- **I/O 없음** — PTY/SSH/소켓 안 읽음. 호출자가 바이트를 `feed()` 로 넣는다.
- **IPC 없음** — Tauri/채널/전송 안 함. 바이너리 *포맷*은 제공하되 *전송*은 소비처 몫.
- **렌더링 없음** — core 는 GPU/캔버스/그리기 안 함. 패밀리의 first-party 렌더러 `justerm-renderer` 가 그린다
  (별도 크레이트 — core 경계는 불변: 여전히 화면 상태·damage 만 노출하고 안 그림).
- **theme 무지(theme-agnostic)** — 색을 *참조*(Default / Indexed(u8) / Rgb)로만 저장. 팔레트→실제
  색 해석은 *소비처/렌더러* 가 frozen 스킴으로. justerm 은 hex 색을 영영 모른다.

→ 결과: PTY 도 Tauri 도 GPU 도 없이 **독립 테스트 가능**(vttest + 단위테스트).

**core 냐 소비처냐 (라우팅 규칙, ADR-0017)**: 기능의 *메커니즘*은 ① VT-파싱이거나 ② 올바르려면 *버퍼
전체*(전 셀·스크롤백·좌표·wrap·wide-char)가 필요하면 **core**(frame 모드 소비처는 뷰포트만 쥐어 물리적으로
못 함) — 단 *정책*(query·regex·palette)은 소비처가 주입해 core 는 policy/theme-agnostic 유지(**메커니즘
core, 정책 소비처**). 그 외(색해석·hover·픽셀→셀·debounce·스크롤바·클립보드·전송)는 소비처. 자세히는 ADR-0017.
*우회 금지*(다른 층 결함을 소비처에서 덮지 말 것)를 포함한 작업 규율은 아래 `### theflow`.

## 기술 스택

- Rust (edition 2024). 핵심 의존성: **`vte`** (Paul-Williams ANSI 파서 — *진짜 어려운 파싱*만 안정
  크레이트에 위임, 그 위 grid/스크롤백/selection 은 자작). `alacritty_terminal` 은 *의존 안 함*
  (API 불안정) — 단 모델 설계의 *참고*. 자세한 근거는 docs/adr/.

## 개발 명령어

```bash
cargo test --workspace   # 코어(justerm-core) + justerm-wasm-decode 바인딩까지 게이트 (--workspace 필수)
cargo bench              # throughput 마이크로벤치(추세 기록)
```

루트는 가상 매니페스트(`[package]` 없음)라 `--workspace` 로 멤버를 명시 게이트해야 하고, 그마저도
`fuzz`·`justerm-facade`·`justerm-renderer`·wasm32-전용 테스트(`justerm-wasm-decode/tests/web.rs`)는
*빌드조차 안 한다*. 크레이트별 전체 게이트 매트릭스(사각 포함)·크레이트 맵은
**`docs/agents/theflow.md` § "Step 7 — gate matrix + downstream loop"**.

## 핵심 규칙

- **주석**: 영어 (코드 주석은 영어로 작성한다).
- **CONTEXT.md / docs/adr/**: 영어 (LLM 토큰 효율). 그 외 사람이 읽는 문서·CLAUDE.md: 한국어.
- **네이밍**: Rust 관용(snake_case 함수/모듈, CamelCase 타입).
- **커밋 메시지**: 관련 GitHub 이슈 번호 참조 (`feat: ... (#12)`). `Co-Authored-By` trailer 금지.
- **컴플라이언스는 누적**: VT 정합성(8.6K SLoC급 long tail)은 한 방에 못 짠다 — 공통 90% 부터,
  dogfood 가 깨는 케이스를 만나며 tail 을 키운다. *뼈대(계약/경계)는 처음부터 옳게*.

## Agent skills

### teach 코스 (개인 학습)

사용자가 `/teach` 로 이 코드베이스를 배우는 다세션 코스가 있다. `/teach` 를 쓸 땐 **먼저 `teach/README.md`
를 읽어라** — 워크스페이스 위치·규칙(그 폴더에서 실행·쓰면 커밋)·진행상황이 거기 있다.

### Issue tracker

Issues are tracked as GitHub issues via the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

Default vocabulary — each triage role's label equals its name. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context — one `CONTEXT.md` + `docs/adr/` at the repo root. See `docs/agents/domain.md`.

### Releasing

태그 구동 + CI 발행 — `vX.Y.Z` 태그 push 가 crates.io + npm 을 *자동* 발행한다(수동 `cargo publish`/`npm publish` 금지, 충돌남). 버전·semver·GitHub Release 규약은 `docs/agents/release.md`.

### Supply-chain check

CI 의 `supply-chain` 게이트는 **just-shield**(같은 소유자=first-party, SHA 핀 된 GitHub Actions 공급망 스캐너; 소스는 형제 repo `../just-shield`)로 워크플로를 `scan --strict` 한다. *결정*은 ADR-0006, *운영*(로컬 재현·R1~R10 규칙 해독·실패 대처)은 `docs/agents/supply-chain.md`.

### theflow — 작업 규율 (그 flow)

**모든 substantive 변경(core·wasm·web·renderer)은 `theflow` 스킬의 7단계 규율로 짠다** — ① 참조·선례·
외부/레지스트리 사실 실측 대조(추측 금지) → ② 경계(메커니즘 core / 정책 소비처; 계약≠결함, 막은 양방향
누수) → ③ 순수 로직 `/tdd`(RED→GREEN) + test-trust 게이트(fix off→red, right reason) → ④ real 왕복
증명(가짜 백엔드 아님; 최강 증명=실 소비처 penterm 링크, DoD ④) → ⑤ adversarial 2렌즈(subagent) → ⑥
behavior 서술 표면 sweep(docs.rs doc-comment·릴리스 노트·**발행 README**·glossary·ADR *근거*·types.ts·
**Epic 본문+라벨**, stale rationale 회수) → ⑦
게이트 전부 → PR/머지, 릴리스 후 downstream loop(소비처 workaround 제거·bug-pin 테스트 flip). 스킬은
방법론까지 소유한다: 1원리+명명된 prior-art 교차검증, "확인 못 함 ≠ 없음"(미확인=갭, cleared=validity 조건),
결정 유형 라우팅(grill 은 제품·정체성 판단만), DoD 4조건, 이슈=durable 기록(defer·거부한 대안·negative
result 를 선행 기록), 검증한 것만 보고, *우회 금지*(다른 층 결함을 소비처에서 보정 말고 멈춰서 사용자에게).
스킬은 이제 `disable-model-invocation`(=`/theflow` 커맨드; 이 포인터가 상시 트리거) — 바인딩 doc 은
`/grill-the-flow` 가 authoring 한다. justerm *바인딩*(크레이트/소비처 맵·참조 라우팅표·경계 구체값·증명수단·
표면 목록·게이트 매트릭스·downstream 절차·실증 이슈 인덱스)은 **`docs/agents/theflow.md`** — 스킬이 런타임에 읽는다.
