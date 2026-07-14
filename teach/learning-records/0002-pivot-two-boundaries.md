# 0002 — 피벗 발견: 정체성은 둘(core 경계 불변 + 패밀리 스코프 변신)

날짜: 2026-07-11

## 계기
레슨 1 을 본 사용자가 스스로 물음: "우리가 초기엔 터미널 엔진만 하려 했는데 xterm 처럼 렌더링까지
통합적으로 담당하는 걸로 변신했다. 내가 읽은 repo 문서에 이게 적혀 있나?" — 즉 레슨의 "렌더링 없음"
프레이밍이 최신인지 검증 요청.

## 실측 확인 (추측 금지 — ADR 원문 직접 읽음)
- **ADR-0002**: beamterm(외부) 채택.
- **ADR-0012**: first-party 렌더러 *방향* 확정, 구현 deferred. "engine boundary unchanged."
- **ADR-0018** (최신, proposed 2026-07-07, ADR-0002 supersede): `justerm-renderer` 빌드 결정.
  - "justerm **pivoted to a first-party full-stack terminal**." / "move from 'engine library' quadrant
    into 'first-party full-stack' quadrant — a **vision choice**." (xterm.js·VS Code 를 참조 사분면으로)
  - "**The engine boundary holds. justerm-core still does NOT render.** The change is the renderer's
    **provenance** (third-party→first-party), not the core's boundary invariant."
- 크레이트 실측: core·renderer·web·wasm-decode·facade 5개 존재.
- CLAUDE.md line 26 은 지금도 "beamterm 이 그린다" — ADR-0018 이 *의도된 지연*이라 명시(#274, OPEN 확인).

## 비-자명한 통찰 (이게 사용자가 내재화해야 할 것)
1. **경계는 둘이다.** ① core↔렌더러 내부 경계 = **불변**(core 안 그림). ② 패밀리 스코프 = **바뀜**
   (엔진 라이브러리 → 풀스택, 렌더러가 외부→자체). 사용자의 "변신" 직감은 ②에 대해 정확.
2. **리뷰어 함정**: "저스텀은 안 그린다"를 순진하게 적용하면 `justerm-renderer` PR 의 정상 코드를 거부하게 됨.
   진짜 불변식은 **스코프가 있다** — core 는 안 그림, renderer 는 그리되 theme-agnostic(팔레트 주입).
   → 리뷰 첫 질문은 "어느 크레이트냐".
3. **문서 읽는 법 메타-교훈**: 이 repo 에서 "지금 뭘 하는 놈이냐 / 왜 바뀌었나"의 최신 진실은
   **CLAUDE.md 본문이 아니라 최신 ADR**에 있다. CLAUDE.md 는 전환 완료까지 의도적으로 옛말을 함(#274).

## 레슨에 반영한 것
- 레슨 1: 파이프라인 그림을 `justerm-core` + "렌더러(beamterm→justerm-renderer, 전환 중)"로 수정.
  §3 을 "core 기준"으로 스코프 명시. **§7 "정체성은 둘" 신설** + 리뷰어 함정 콜아웃 + 문서-지연 콜아웃.
  퀴즈에 **Q5**(renderer 가 픽셀 그림 = 정상) 추가.
- 용어집: beamterm·justerm-renderer·피벗·패밀리 엔트리 추가, 소비처 엔트리 정정.

## 교육적 가치
사용자가 스스로 "문서에 적혀 있나?"를 물어 검증을 요구한 것 자체가 좋은 리뷰어 본능 — 이건 CLAUDE.md
"확인 못 했다 ≠ 없다" 규율과 같은 방향. 앞으로 레슨에서 이 습관(주장→ADR 원문 대조)을 계속 강화.

## 다음 후보 (ZPD) — 갱신
- 레슨 3 후보 급부상: **"어느 크레이트냐" 라우팅** — ADR-0017(메커니즘 core, 정책 소비처) + 이번 피벗의
  crate-스코프 경계. 레슨 1 §7 의 자연스러운 심화이고 사용자가 방금 관심 보인 지점.
- 레슨 2(셀·damage)는 그 다음.
