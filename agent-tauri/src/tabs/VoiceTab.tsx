import { Badge, Button, Card, TextInput } from '../components/ui';

/**
 * Proximity Voice Chat 연동 준비 탭.
 * 백엔드(시그널링·WebRTC·위치 추적)는 아직 에이전트에 통합되지 않았으므로
 * 전체 컨트롤을 비활성 스켈레톤으로 표시한다. (참고: Waitence7/ProximityVoiceChat)
 */
export function VoiceTab() {
  return (
    <div className="space-y-3">
      <Card
        title="근접 보이스 챗 (Proximity Voice Chat)"
        action={<Badge tone="accent">곧 지원 예정</Badge>}
      >
        <p className="text-[12px] leading-relaxed text-zinc-400">
          내전(커스텀 게임)에서 미니맵 거리 기반으로 팀원 목소리 크기가 달라지는 근접 보이스
          기능이 곧 추가됩니다. 게임 화면의 미니맵에서 위치를 읽어 서버가 거리별 볼륨을
          계산합니다 — 메모리 조작이나 인젝션은 사용하지 않습니다.
        </p>
      </Card>

      <fieldset disabled className="space-y-3 opacity-45">
        <Card title="방">
          <div className="flex items-center gap-2">
            <Button variant="primary">방 생성</Button>
            <TextInput placeholder="참가 코드" className="max-w-36 text-center font-mono" />
            <Button>참가</Button>
          </div>
        </Card>

        <Card title="오디오 장치">
          <div className="grid grid-cols-2 gap-3">
            <label className="block">
              <span className="mb-1 block text-[12px] text-zinc-400">마이크</span>
              <DeviceSelect placeholder="기본 마이크" />
            </label>
            <label className="block">
              <span className="mb-1 block text-[12px] text-zinc-400">출력</span>
              <DeviceSelect placeholder="기본 출력" />
            </label>
          </div>
          <div className="mt-3 flex items-center gap-2">
            <Button>마이크 ON</Button>
            <Button>전체 켜짐</Button>
            <label className="ml-2 flex items-center gap-1.5 text-[12px] text-zinc-400">
              <input type="checkbox" className="accent-indigo-500" /> PTT (V)
            </label>
          </div>
        </Card>

        <Card title="참가자">
          <p className="text-[12px] text-zinc-500">방에 참가하면 참가자 목록이 표시됩니다.</p>
        </Card>
      </fieldset>
    </div>
  );
}

function DeviceSelect({ placeholder }: { placeholder: string }) {
  return (
    <select className="w-full rounded-lg border border-white/10 bg-zinc-950/60 px-3 py-2 text-[12px] text-zinc-400">
      <option>{placeholder}</option>
    </select>
  );
}
