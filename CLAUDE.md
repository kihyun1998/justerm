# justerm

VT 바이트 스트림을 터미널 화면 상태(그리드 + 스크롤백)로 짜 넣는 **순수 터미널 엔진** (Rust).
렌더러도 emulator 도 아니다 — 화면을 *그리지 않고*, 화면 *상태와 변경분(damage)* 을 만들어 노출한다.

- **엔진 = `justerm-core`** (이 repo 의 코어 크레이트, 파싱+상태) / **렌더러 = `beamterm`** (그리드를
  WebGL2 로 그림, 별도) → `-term` 패밀리. `justerm` 은 *패밀리 umbrella* 이름이다(코어 `justerm-core` +
  wasm 디코더 `justerm-wasm-decode` + 향후 `justerm-web`) — v0.6.0 에서 맨이름 `justerm` 을 개명(ADR-0010).
- **첫 소비처 = PenTerm** (Tauri 터미널 앱). `justerm-core` 는 penterm 전용이 아니라 *재사용 가능한 독립
  크레이트*다.
- **상세 계약(구현 시 참조)**: **`docs/architecture.md`** — 셀·damage·뷰포트/스크롤·cadence·
  selection·직렬화·엔진 API 의 authoritative 스펙. 핵심 결정 근거는 `docs/adr/`(0001 vte·0002
  beamterm). 큰 그림·빌드플랜은 GitHub **Epic #1** + 슬라이스 #2–#12. *이 repo 안에서 전부 참조
  가능* — penterm 안 봐도 됨.
- **설계 출처(역사)**: penterm 의 `.scratch/rust-terminal-engine/PRD.md` — 이 계약이 grill 로
  확정된 원본 기록(2026-06-16, prior-art 교차검증). 근거를 더 파고 싶을 때만 참조.

## 경계 invariant (이게 정체성)

justerm 이 **하는 것**: vte 로 VT 스트림 파싱 → 셀 그리드 + 스크롤백 + 커서 + selection 상태 보유 →
*뷰포트 스냅샷 + damage(줄+열범위) + scroll op* 를 노출. text 추출(복사) 제공.

justerm 이 **하지 않는 것** (의존성으로 끌어들이지도 말 것):
- **I/O 없음** — PTY/SSH/소켓 안 읽음. 호출자가 바이트를 `feed()` 로 넣는다.
- **IPC 없음** — Tauri/채널/전송 안 함. 바이너리 *포맷*은 제공하되 *전송*은 소비처 몫.
- **렌더링 없음** — GPU/캔버스/그리기 안 함. beamterm 이 그린다.
- **theme 무지(theme-agnostic)** — 색을 *참조*(Default / Indexed(u8) / Rgb)로만 저장. 팔레트→실제
  색 해석은 *소비처/렌더러* 가 frozen 스킴으로. justerm 은 hex 색을 영영 모른다.

→ 결과: PTY 도 Tauri 도 GPU 도 없이 **독립 테스트 가능**(vttest + 단위테스트).

**core 냐 소비처냐 (라우팅 규칙, ADR-0017)**: 기능의 *메커니즘*은 ① VT-파싱이거나 ② 올바르려면 *버퍼
전체*(전 셀·스크롤백·좌표·wrap·wide-char)가 필요하면 **core**(frame 모드 소비처는 뷰포트만 쥐어 물리적으로
못 함) — 단 *정책*(query·regex·palette)은 소비처가 주입해 core 는 policy/theme-agnostic 유지(**메커니즘
core, 정책 소비처**). 그 외(색해석·hover·픽셀→셀·debounce·스크롤바·클립보드·전송)는 소비처. 자세히는 ADR-0017.

## 기술 스택

- Rust (edition 2024). 핵심 의존성: **`vte`** (Paul-Williams ANSI 파서 — *진짜 어려운 파싱*만 안정
  크레이트에 위임, 그 위 grid/스크롤백/selection 은 자작). `alacritty_terminal` 은 *의존 안 함*
  (API 불안정) — 단 모델 설계의 *참고*. 자세한 근거는 docs/adr/.

## 개발 명령어

```bash
cargo test --workspace   # 코어(justerm-core) + justerm-wasm-decode 바인딩까지 게이트 (--workspace 필수)
cargo bench              # throughput 마이크로벤치(추세 기록)
```

**`--workspace` 는 필수**: 루트는 가상 매니페스트(`[package]` 없음)라 멤버를 명시 게이트해야 한다 —
공개 API 를 바꾸는 변경은 `justerm-wasm-decode` 바인딩을 조용히 깨뜨릴 수 있으니(0.4.0 에서 발생), 공개
표면을 건드릴 땐 `cargo test --workspace`/`cargo clippy --workspace --all-targets` 로 멤버 전부를 검증한다(CI 와 동일).

**`--workspace` *밖* 사각지대**: `fuzz` 와 `justerm-facade` 는 의도적으로 워크스페이스 밖이다(루트
`[workspace] exclude`; fuzz 는 자체 `[workspace]`, facade 는 버전 lockstep 바깥의 일회성 `justerm` 0.5.1
묘비). `--workspace` 게이트가 이들을 *빌드조차 안 하므로*, 개명·공개경로 변경 후엔
`cargo check --manifest-path fuzz/Cargo.toml` 로 별도 검증한다. 같은 류의 다른 사각:
`cargo fmt --all --check`(핀 버전)와 wasm32-전용 `justerm-wasm-decode/tests/web.rs`(host 에선 0컴파일).

## 핵심 규칙

- **주석**: 영어 (코드 주석은 영어로 작성한다).
- **CONTEXT.md / docs/adr/**: 영어 (LLM 토큰 효율). 그 외 사람이 읽는 문서·CLAUDE.md: 한국어.
- **네이밍**: Rust 관용(snake_case 함수/모듈, CamelCase 타입).
- **커밋 메시지**: 관련 GitHub 이슈 번호 참조 (`feat: ... (#12)`). `Co-Authored-By` trailer 금지.
- **컴플라이언스는 누적**: VT 정합성(8.6K SLoC급 long tail)은 한 방에 못 짠다 — 공통 90% 부터,
  dogfood 가 깨는 케이스를 만나며 tail 을 키운다. *뼈대(계약/경계)는 처음부터 옳게*.

## 사고방식

아키텍처/설계 결정은 **1원리 도출 + 명명된 prior-art(Mosh·Alacritty·Warp·VS Code·beamterm) 교차
검증**을 함께 한다. 수렴 = 비임의성 신호, prior art 가 1원리의 under-reach 디테일을 깎는다.
"완벽 = 최대 granular" 아님 — *올바른 grain*. 자세히는 메모리 참조.

이건 *design* 뿐 아니라 **VT-semantics 구현(#2·#3·#4·#6·#7·#10)에도 적용**한다 — 1원리/계약만 보면
*correct-looking 한데 숨은 상태를 빠뜨린* 모델이 나온다(pending-wrap·wide-char spacer·BCE 가 그 예).
**구현 전, 참조 구현(vte/alacritty/xterm)이 그 영역에서 추적하는 *숨은 상태*를 열거**하고
`docs/architecture.md` § "Hidden VT state" 에 추가하라. 체계적 catch 는 #7 vttest — 일찍 세워라.

**참조 소스는 `gh api` 로 raw 를 직접 받아 grep/sed 한다** (WebFetch 금지). WebFetch 는 요약 모델이
큰 파일을 잘라 *메서드 본문을 놓친다*(예: xterm.js `InputHandler.ts` 3.7K 줄 — 등록부만 보이고
`setOrReportIndexedColor` 등 핸들러 본문은 잘림). 대신
`gh api repos/<owner>/<repo>/contents/<path> --jq .content | base64 -d > /tmp/x.ts` 로 전체를 받아
`grep -n`/`sed -n` 으로 *실제 줄* 을 읽어라. 기억/요약 아닌 실코드 대조가 원칙(메모리
`feedback_proactive_adversarial_and_reference_verify`).

**결정 유형으로 라우팅한다.** 순수 기술 메커니즘(와이어 포맷·좌표계·API 모양 등 — 코드 + 명명된
prior-art 로 *도출 가능*한 것)은 사용자에게 grilling 하지 말고 **직접 결정 → 실제 소스 대조 검증 →
결과만 제시**(yes/no 승인). 답이 코드에 있는 걸 묻는 건 일 떠넘기기다. grilling/질문은 **제품·정체성·
우선순위 판단**(네이밍·repo 구조·스코프·비전 — 사용자가 의견 갖고 교정할 영역)에만 쓴다. ADR 은 보통
*구현 시* 확정이 더 단단하다(실제 encode/decode 디테일이 결정을 firm up).

**완성 기준 — "대충 금지".** 슬라이스/기능 "완료" 는 ① 골격(계약·경계)이 처음부터 옳고 ② 로직이 100%
테스트되고 ③ 갭이 *추적된 0*(남기는 deferral 은 전부 이슈로 surfacing — *침묵하는* 갭 0) ④ 동작 증명이
*자기 가짜가 아닌 실 core/참조(xterm.js·alacritty)* 왕복으로 설 때만이다. 데모/테스트의 fake 백엔드는
feel용 스모크일 뿐 *증명이 아니다* — fake 가 통과해도 real 이 다를 수 있으니, 한 슬라이스당 최소 1회는
실 core(encode→decode 왕복, ADR-0005) 또는 참조 구현으로 가정을 교차검증한다. "결과만 보고 넘어가기" 는
②④ 위반. 이건 `컴플라이언스는 누적`(갭을 허용하되 *추적*)·메모리 `reference-verify`(가짜 아닌 참조로
검증, 품질을 사용자 닦달에 의존 금지)의 강화판이다.

**Adversarial 검증은 subagent 로 (자기 enumeration 을 불신).** 반응적 spike(엣지를 하나씩 손으로 탐침)가
*계속* 새 갭을 잡으면, 그건 "운 나쁨" 이 아니라 *내 숨은상태 enumeration 이 불완전*하다는 신호다 — 더
무작위로 찌르지 말고 **독립 completeness 비평가(subagent)를 *서로 다른 렌즈*로 병렬** 돌린다. 렌즈 예:
① 이 repo 자체 — `architecture.md` §"Hidden VT state" + 형제 구현(search/selection 등) 셀-walk diff, ②
참조구현 — xterm/alacritty 실소스(`gh api` raw) 대조. 각 에이전트는 숨은상태/엣지 매트릭스를 *체계적으로
열거*해 (a) 남은 갭을 한 번에 surfacing 하거나 (b) *수렴*(나머지 전부 covered)을 *증명*한다 — 무작위
spike 와 달리 **끝(멈출 confidence)이 보인다**. 서로 다른 렌즈가 *내가 그 렌즈와 공유하는 blind spot* 을
깬다. 실증(#113 logical-lines): 단일-버퍼만 보던 내가 못 본 **alt-screen cross-buffer 결함**을 잡고,
`search()` 의 기존 동일 버그까지 surfacing(#144), 동시에 두 렌즈가 무한-walk 에서 독립 수렴 + justerm이
xterm보다 *옳은* 지점까지 검증. DoD ④(가짜 아닌 참조 검증)의 실행 수단이고, 결과(갭→TDD 수정, 수렴→커밋)는
내가 main loop 에서 받아 처리한다.

## Agent skills

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

## 작업 flow (core·wasm·web 공통 — "그 flow")

**이건 `justerm-web` 전용이 아니다.** core(justerm-core)·wasm(justerm-wasm-decode)·web(justerm-web) 어느 크레이트든 *substantive 변경*이면 이 6단계로 짠다. 단계를 *생략*하려면 (건너뛰는 게 아니라) *왜 이 변경엔 해당 없는지를 명시*한다 — 유형이 다르면 *어느 참조를 보느냐*가 달라질 뿐, "본다·비교한다·검증한다"는 안 바뀐다. (web 슬라이스는 S8/#109 에서 이 형태로 확립됨.)

1. **참조·선례 실측 대조 먼저 (추측 금지, 변경 유형으로 라우팅).** `gh api …/contents/<path> --jq .content | base64 -d` 로 *통째* 받아 grep/sed 로 실코드를 읽는다(WebFetch 금지). 상수·모드명·숨은동작을 기억이 아니라 실코드/선례로 박는다.
   - **web 기능** → **xterm.js 실소스**(`repos/xtermjs/xterm.js`). 예: drag-scroll 50px/15, highlightLimit 1000, `_charsToConsume`.
   - **core VT-semantics** → **xterm/alacritty 실소스** + *이 repo 형제 구현*(`docs/architecture.md` §"Hidden VT state" + search/selection 셀-walk). 참조가 추적하는 *숨은 상태*를 먼저 열거(pending-wrap·wide-char spacer·BCE 류).
   - **wire/포맷·좌표계·API 모양** → *이 repo 형제 필드/선례*(#129 mouse_events·#112 scroll·#108 overlay 가 struct→encode→decode→Flat→getter→types.ts 를 어떻게 touch 했나) + **ADR**(0013/0014 = 헤더에 뷰포트 상태 싣기, 0008 = decode 경계). 새 wire 필드는 *가장 최근 형제를 그대로 미러*한다.
2. **경계를 코드로 가른다 (ADR-0017: 메커니즘 core, 정책 소비처).** 기능의 메커니즘이 ①VT-파싱이거나 ②*버퍼 전체*가 필요하면 **core**; 정책(query·regex·palette·announce 정책)은 소비처가 주입. web 은 frame 모드에서 엔진을 *안 돌린다* — 상태·텍스트·word경계는 core 가, web 은 *명령을 보내고*(write seam = `FrameSource` 의 형제 `SelectionPort`/`SearchPort`…, 쿼리는 `Promise` IPC) 프레임 overlay 를 *그린다*(scrollback 셀이 없어 계산 불가 = 경계의 물리적 강제).
3. **순수 로직을 `/tdd` 로 (RED→GREEN 수직, 한 번에 하나).** 부수효과(port·clock/`tick()`·clipboard·scroll·DOM)는 *주입 seam* 으로 받고 `MouseEventLike` 같은 *구조적* 타입으로 단언(실 DOM/이벤트가 그대로 만족). web 컨트롤러는 DOM/GPU/IPC **0 의존**, 공개표면은 `src/index.ts`. *wire 필드처럼 컴파일이 완전성을 강제*하는 변경은 엄격 RED 가 어색하니 round-trip 테스트가 그 자리를 대신하되, **테스트가 impl 을 뒤따르지 않게** 새 동작 테스트는 먼저 쓴다.
4. **동작 증명 — 가짜 아닌 real 왕복(DoD ④).** fake 백엔드/데모는 feel용 스모크일 뿐 *증명 아님*. 유형별 real:
   - **web** → `pnpm demo` 실브라우저(DPR·좌표·렌더 버그) + 필요시 실 스크린리더/HITL. (캔버스 버퍼=CSS×DPR, geometry 는 화면 박스 역산 `rect.h/ROWS`.)
   - **core/wasm** → `encode→decode` 왕복(ADR-0005)·`vttest`·**실 PTY 캡처**(사용자 RHEL VM, 메모리 참조) 로 가정 교차검증.
5. **Adversarial completeness 패스 (subagent 2렌즈) — 성질로 판단.** 변경이 *내 숨은상태 enumeration 이 불완전할 수 있는* 성질(엣지/상태 다수, VT-semantics, 상호작용)이면 **필수**: 독립 비평가를 *서로 다른 렌즈*(① 이 repo 형제 ② 참조구현 xterm/alacritty)로 병렬 → 갭 surfacing 또는 *수렴 증명*. 반대로 *닫힌 표면*(컴파일러+round-trip 이 완전성을 exhaustive 하게 게이트하는 순수 기계적 변경)이면 생략 가능하나 **그 판단을 명시 기록**(생략 자체가 침묵 갭이 되지 않게). "web 이냐 core 냐"가 아니라 *enumeration 리스크가 있냐* 로 건다 — demo/spike 가 갭을 *연달아* 잡으면 그게 트리거.
6. **게이트 & PR/머지.** 크레이트별 게이트 *전부*(사각 포함):
   - **core/wasm** → `cargo test --workspace` + `cargo fmt --all --check` + `cargo clippy --workspace --all-targets` + `cargo check --manifest-path fuzz/Cargo.toml`(워크스페이스 밖) + `cargo build -p justerm-wasm-decode --tests --target wasm32-unknown-unknown`(wasm32-only `web.rs` 는 host 에서 0컴파일 — *런타임 단언*은 브라우저 CI 에서만; 버전-핀 테스트를 host·wasm 양쪽 갱신).
   - **web** → `pnpm typecheck` + 전체 vitest + `pnpm demo`.
   - 브랜치 → `feat(<scope>): … (#issue)`(Co-Authored-By 금지) → squash PR(`Closes #issue`) → `test`/`wasm` CI 그린 확인.
