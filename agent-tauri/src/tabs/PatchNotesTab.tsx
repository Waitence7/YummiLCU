import { Badge, Card } from '../components/ui';

type NoteGroup = {
  title: string;
  tone: 'accent' | 'ok' | 'warn' | 'neutral';
  items: string[];
};

const V072_NOTES: NoteGroup[] = [
  {
    title: '0.7.2 자동 업데이트 복구',
    tone: 'ok',
    items: [
      '코드서명 인증서가 설정되지 않은 stable 빌드에서도 Ed25519로 서명된 업데이트 manifest와 SHA-256 파일 검증을 이용해 자동 업데이트를 계속할 수 있도록 수정했습니다.',
      'Windows publisher thumbprint는 더 이상 stable 업데이트의 필수 조건이 아니며, 설정된 경우에만 Authenticode 추가 검증으로 사용합니다.',
    ],
  },
];

const V071_NOTES: NoteGroup[] = [
  {
    title: '0.7.1 핫픽스',
    tone: 'warn',
    items: [
      'WebView2에서 HTML-in-Canvas 실험 기능이 노출되지 않던 문제를 수정하기 위해 CanvasDrawElement를 Blink runtime feature로 직접 활성화하도록 변경했습니다.',
      'HTML-in-Canvas 진단에 texElementImage2D, drawElementImage 노출 여부와 WebView 사용자 에이전트를 함께 기록해 런타임 호환성 원인을 더 정확히 확인할 수 있게 했습니다.',
    ],
  },
];

const V070_NOTES: NoteGroup[] = [
  {
    title: '새 기능',
    tone: 'accent',
    items: [
      'Discord Rich Presence를 확장해 현재 플레이 상태를 더 자연스럽게 표시하고, 지원되는 상황에서 파티 참가 흐름을 제공합니다.',
      '트레이로 전환할 때 사용할 수 있는 HTML-in-Canvas 애니메이션과 「유미의 책」 효과를 추가하고, V2에는 입체 표지·책등·페이지 단면과 Blender 레퍼런스 디테일을 반영했습니다.',
      '닫기 버튼으로 트레이에 보낼 때 유미의 책 V2 애니메이션과 전용 사운드가 함께 재생되도록 했습니다.',
      '로그 화면에서 진단 정보를 확인하고 파일로 내보낼 수 있는 진단 도구를 추가했습니다.',
    ],
  },
  {
    title: '내전 · LCU',
    tone: 'ok',
    items: [
      '진행 중인 게임 이벤트와 타임라인 전달을 개선해 웹과 Discord의 내전 상태 동기화를 더 안정적으로 만들었습니다.',
      '관전 상태 감지와 라이브 게임 데이터 처리를 보강하고, 불완전한 게임 정보가 전달되는 경우를 줄였습니다.',
      'LCU 이벤트 재연결과 상태 추적을 개선해 클라이언트 재시작이나 연결 변화에도 상태가 더 빠르게 복구됩니다.',
    ],
  },
  {
    title: '업데이트 · 앱 동작',
    tone: 'neutral',
    items: [
      'stable / beta / dev 릴리스 채널과 실제 빌드 식별자를 분리해 설치된 빌드를 더 명확하게 확인할 수 있습니다.',
      '중단된 자동 업데이트 복구와 배포 경로 호환성을 개선하고, beta 설치 파일을 앱에서 바로 확인할 수 있게 했습니다.',
      '단일 인스턴스와 트레이 창 동작을 정리해 중복 실행과 창 전환 과정의 예외 상황을 줄였습니다.',
    ],
  },
  {
    title: '보안 · 안정성',
    tone: 'warn',
    items: [
      'Relay에서 전달되는 명령의 검증을 강화하고 LCU 연결·명령 처리 경계를 더 엄격하게 제한했습니다.',
      'League lockfile 탐색과 관리형 PC 환경 대응을 보강했습니다.',
      'Relay 오류 모니터링과 진단 정보를 확장해 연결 문제의 원인을 더 쉽게 확인할 수 있습니다.',
      'stable에서도 HTML-in-Canvas를 활성화하고 유미의 책 V2를 기본 트레이 효과로 사용합니다. 지원되지 않는 WebView2에서는 자동으로 호환 효과로 전환됩니다.',
    ],
  },
];

export function PatchNotesTab() {
  return (
    <div className="space-y-3">
      <Card className="overflow-hidden border-indigo-100 bg-gradient-to-br from-white to-indigo-50/50">
        <div className="flex items-start justify-between gap-3">
          <div>
            <div className="flex items-center gap-2">
              <h2 className="text-[18px] font-semibold tracking-tight text-slate-900">Yummi LCU Agent 0.7.2</h2>
              <Badge tone="ok">정식 릴리스</Badge>
            </div>
            <p className="mt-1 text-[11px] leading-relaxed text-slate-500">
              내전 연동, Discord 상태 표시, 업데이트 경험과 트레이 애니메이션을 중심으로 다듬은 업데이트입니다.
            </p>
          </div>
          <span className="shrink-0 rounded-lg bg-white/80 px-2.5 py-1 text-[10px] font-medium text-slate-500 shadow-sm ring-1 ring-slate-200/70">
            0.7.2
          </span>
        </div>
        <div className="mt-3 rounded-lg border border-emerald-100 bg-emerald-50/70 px-3 py-2 text-[10px] leading-relaxed text-emerald-700">
          0.7.2 자동 업데이트 복구, 0.7.1 핫픽스와 0.7.0 정식 릴리스 변경사항입니다.
        </div>
      </Card>

      {[...V072_NOTES, ...V071_NOTES, ...V070_NOTES].map((group) => (
        <Card
          key={group.title}
          title={
            <span className="flex items-center gap-2">
              <Badge tone={group.tone}>{group.title}</Badge>
            </span>
          }
        >
          <ul className="space-y-2">
            {group.items.map((item) => (
              <li key={item} className="flex gap-2 text-[11px] leading-relaxed text-slate-600">
                <span className="mt-[7px] size-1.5 shrink-0 rounded-full bg-slate-300" />
                <span>{item}</span>
              </li>
            ))}
          </ul>
        </Card>
      ))}
    </div>
  );
}
