# justerm

VT 바이트 스트림을 터미널 화면 상태(그리드 + 스크롤백)로 짜 넣는 **순수 터미널 엔진** (Rust).
렌더러도 emulator 도 아니다 — 화면을 *그리지 않고*, 화면 *상태와 변경분(damage)* 을 만들어 노출한다.

- **엔진 = justerm** (이 repo, 파싱+상태) / **렌더러 = `beamterm`** (그리드를 WebGL2 로 그림, 별도) →
  `-term` 패밀리.
- **첫 소비처 = PenTerm** (Tauri 터미널 앱). justerm 은 penterm 전용이 아니라 *재사용 가능한 독립
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

## 기술 스택

- Rust (edition 2024). 핵심 의존성: **`vte`** (Paul-Williams ANSI 파서 — *진짜 어려운 파싱*만 안정
  크레이트에 위임, 그 위 grid/스크롤백/selection 은 자작). `alacritty_terminal` 은 *의존 안 함*
  (API 불안정) — 단 모델 설계의 *참고*. 자세한 근거는 docs/adr/.

## 개발 명령어

```bash
cargo build
cargo test          # 단위테스트 + vttest 정합성
cargo bench         # throughput 마이크로벤치(추세 기록)
```

## 핵심 규칙

- **주석**: 한국어.
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

## Agent skills

### Issue tracker

Issues are tracked as GitHub issues via the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

Default vocabulary — each triage role's label equals its name. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context — one `CONTEXT.md` + `docs/adr/` at the repo root. See `docs/agents/domain.md`.
