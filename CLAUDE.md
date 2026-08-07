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
- **착수 전 배선도**: **`docs/map/`** (허브 `docs/map/README.md`) — *"이걸 건드리면 뭐가 같이
  움직이나"*(수평)와 *"이 코드는 어떤 설계·결정에서 나왔나"*(수직)에 답하는 링크 그래프. 아래
  ADR/architecture.md 는 **사건**으로 색인돼 있어(결정한 날·작업·재결정 클러스터) 둘 다 못 답한다.
  영토 노트는 **배타적이지 않고**, N개 영토에 걸치는 사실은 *횡단 불변식* 노트로 승격한다 —
  `abs_floor` 가 네 곳에 걸린 채 아무 데도 안 그려져 있어 세 번 따로 발견된(#113→#144→#207) 실패가
  이 층이 존재하는 이유다. **빈 `## Governing decisions` 는 결함이 아니라 산출물이다.** 유지 규율은
  `docs/agents/theflow.md` Step 6.
- **상세 계약(구현 시 참조)**: **`docs/architecture.md`** — 셀·damage·뷰포트/스크롤·cadence·
  selection·직렬화·엔진 API 의 authoritative 스펙. 핵심 결정 근거는 `docs/adr/`(0001 vte·0002
  beamterm→0018 justerm-renderer·0019 셀 합성 모델 — 렌더러가 셀 하나를 bg/fg/잉크로 푸는
  *전역함수*, xterm 은 validator 아닌 설계 입력). 아래는 최근 ADR 의 *한 줄 라우팅*일 뿐 —
  **status(proposed/accepted)는 여기 안 적는다**: 각 ADR 파일 머리의 `Status:` 줄이 authoritative 이고,
  그걸 여기 복사하면 게이트가 없어 조용히 낡는다(2026-07-22 에 0020–0023 이 accepted 됐는데 이 문단은
  닷새 뒤까지 "최근 4개는 proposed" 라고 말하면서 다섯 개를 나열하고 있었다 — `architecture.md` 가
  엔진 API 목록을 통째로 지운 것과 같은 이유):
  **0020** 프레임 스냅샷에 실릴 자격(상태냐 사건이냐 / 소비처가 이미 쥐었나 / 뷰포트로 유계인가 —
  wire 그룹을 하나 더 얹기 전에 통과해야 하는 3규칙), **0021** 전역 WebGL2 컨텍스트 1개가 N 그리드를
  뷰포트로(`TerminalSurface`, 자원 3층 + 층 배정 규칙; #287), **0022** 셀 = 폰트 `█` 의 잉크 박스와
  거기서 파생되는 모든 기하(측정 방식은 beamterm 물림, 근거 미검증으로 등급 표시),
  **0023** 간격 설정의 단위는 CSS px(=`font_size` 와 같은 공간; 양 레퍼런스는 device px 라 한 폰트 서술이
  두 단위를 말함), **0024** decoration 은 *색 + 마크*이지 객체가 아님 → 투영/precedence 규칙 6개가
  거기서 파생(셀=등록순서, ruler=클래스 먼저; ADR-0019 가 out of scope 로 밀어낸 축),
  **0025** row/wide-pair 상태는 주인 하나 + 생명주기 하나이지 verb 마다의 규칙이 아님(`Cell` 은
  셀 단위로 쓰이는데 wrap 링크는 *행*, spacer 마커는 *pair* 의 사실이라 생기는 어긋남 — 이 영역의
  새 질문은 D1–D4 에 대한 conformance 로 다룬다; 로스터는 spine #552),
  **0026** 밖에서 들어온 좌표는 *한 번* 잡히고 리더가 짝의 한쪽만 잡지 않는다 — 어느 표면이 잡느냐는
  엔진이 그 좌표의 producer 를 가졌는지에서 파생된다(포인터는 write-site, 소비처가 저술한 `Match` 는
  projection). 레퍼런스가 clamp/hide 로 1–1 갈려서, 타이는 "justerm 의 옛 동작이 둘 중 어느 것도
  아니었다"로 깨졌다(#660→#671→#678),
  **0027** liveness 질문은 답을 *소유한* 소스가 답한다 — 쥔 호출자는 쥔 값을, 물어야 하는 호출자는
  어긋날 수 있는 소스 전부를, 소비처에 공개되는 값은 *보고*이지 우리 술어가 아니다. 브라우저가
  컨텍스트를 동기적으로 죽이고 이벤트만 큐잉해서 "lost 인가"에 동시에 참인 답이 둘이라 생기는 문제
  (#639→#688→#695; spine #689 승격·종료). 따름정리가 구조적이다 — *보고만 볼 수 있는 모듈은 보고
  질문만 답할 수 있다*(`context_loss.rs` 가 프레임 결정을 혼자 못 내리는 이유).
  **0028** IME composition 이 화면에 올리는 것 — 그것이 건드리는 표면마다 writer 는 정확히 하나이고,
  브라우저 소유권이 *가시성*까지 미치지는 않는다(spine #640 승격; ADR-0019 Totality 개정을 강제 —
  preedit 은 글리프를 *공급*한다).
  **0029** 좌표가 core 를 떠날 땐 *언제 기준인지*를 같이 지고 나가거나, **다시 물으면 항상 답이 나오는
  질의**여야 한다 — 의무를 갚는 길이 둘이고, 어느 쪽인지는 표면에서 *파생*된다(소비 clock 이 다시 물을
  수 있나 + 기준 버퍼가 고정인가). 그래서 `marker_index` 는 basis+epoch 를 싣고 `command_marks` 는
  둘 다 없이도 옳다. event 는 선택지가 없어 항상 싣는다(#490→#737→#741→#742; spine #744 승격·종료).
  *어느 버퍼냐* 축은 여전히 **범위 밖**이다 — 다만 #743 이 그 축과 만나는 유일한 멤버
  (`CommandLine::line`: 문서 좌표라 지금 싣는 스칼라로는 못 대는 축이 둘)를 처리하면서, D3.2 가 *어느 버퍼냐* 의
  답 하나를 **배제**한다는 게 드러났다(alt 에서 빈 답을 주면 부재가 "폐기"와 "다른 화면" 으로 갈려
  re-ask 자격을 잃음). 배제는 하되 선택은 여전히 안 한다.
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
- **CONTEXT.md / docs/adr/ / docs/map/**: 영어 (LLM 토큰 효율 — `docs/map/` 은 *모든* 착수 시점에
  에이전트가 읽으므로 glossary 와 같은 근거가 적용된다). 그 외 사람이 읽는 문서·CLAUDE.md: 한국어.
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
증명(가짜 백엔드 아님; 최강 증명=실 소비처 penterm 링크, DoD ④) → ⑤ adversarial 완전성 패스(subagent
1개가 형제+참조 *양쪽* corpus 를 읽는다 — corpus 로 쪼개면 방향 판정을 못 해 메인 스레드로 넘어온다;
2026-07-24 합침. 반증 렌즈 1개 추가는 무조건 트리거 3개에서만) → ⑥
behavior 서술 표면 sweep(docs.rs doc-comment·릴리스 노트·**발행 README**·glossary·ADR *근거*·types.ts·
**Epic 본문+라벨**, stale rationale 회수) → ⑦
게이트 전부 → PR/머지, 릴리스 후 downstream loop(소비처 workaround 제거·bug-pin 테스트 flip). 스킬은
방법론까지 소유한다: 1원리+명명된 prior-art 교차검증, "확인 못 함 ≠ 없음"(미확인=갭, cleared=validity 조건),
결정 유형 라우팅(grill 은 제품·정체성 판단만), DoD 4조건, 이슈=durable 기록(defer·거부한 대안·negative
result 를 선행 기록), 검증한 것만 보고, *우회 금지*(다른 층 결함을 소비처에서 보정 말고 멈춰서 사용자에게).
스킬은 이제 `disable-model-invocation`(=`/theflow` 커맨드; 이 포인터가 상시 트리거) — 바인딩 doc 은
`/grill-the-flow` 가 authoring 한다. justerm *바인딩*(크레이트/소비처 맵·참조 라우팅표·경계 구체값·증명수단·
표면 목록·게이트 매트릭스·downstream 절차·실증 이슈 인덱스)은 **`docs/agents/theflow.md`** — 스킬이 런타임에 읽는다.
레퍼런스(alacritty·ghostty·xterm.js)는 **`../.refs/` 의 SHA 핀된 로컬 체크아웃을 `rg` 로** 읽는다(`gh api` 아님 — 8줄 사실에
파일 통째를 받느라 렌즈 한 패스가 20~30분이었다). 이미 확인된 사실은 **`docs/agents/reference-facts.md`** 에 `file:line`+SHA 로
쌓이니 백지에서 시작하지 말 것. 트리 생성·핀 갱신 절차는 theflow.md § "Step 1".
