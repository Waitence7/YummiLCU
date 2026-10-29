import { useEffect, useState } from "react";

import * as api from "../api/commands";
import { Button, Card } from "../components/ui";
import type { OverlayInstallStatus } from "../state/types";

function Key({ children }: { children: string }) {
  return (
    <kbd className="rounded border border-slate-300 bg-white px-1.5 py-0.5 font-mono text-[10px] font-semibold text-slate-700 shadow-sm">
      {children}
    </kbd>
  );
}

function GuideStep({
  number,
  title,
  children,
}: {
  number: number;
  title: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex gap-3 rounded-lg border border-slate-200 bg-slate-50 px-3 py-3">
      <span className="flex size-6 shrink-0 items-center justify-center rounded-full bg-indigo-600 text-[11px] font-bold text-white">
        {number}
      </span>
      <div className="min-w-0">
        <strong className="block text-[12px] text-slate-900">{title}</strong>
        <div className="mt-1 text-[10px] leading-relaxed text-slate-600">
          {children}
        </div>
      </div>
    </div>
  );
}

export function OverlayTab() {
  const [status, setStatus] = useState<OverlayInstallStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [installing, setInstalling] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = async () => {
    setLoading(true);
    setError(null);
    try {
      setStatus(await api.getOverlayInstallStatus());
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void refresh();
  }, []);

  const install = async () => {
    setInstalling(true);
    setError(null);
    try {
      setStatus(await api.installOverlay());
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
      void api.reportUiFailure("overlay_install_tab", cause);
    } finally {
      setInstalling(false);
    }
  };

  return (
    <div className="space-y-3">
      <Card
        title="유미지지 오버레이"
        action={
          <Button
            variant="ghost"
            disabled={loading || installing}
            onClick={() => void refresh()}
          >
            새로고침
          </Button>
        }
      >
        <div className="space-y-2.5">
          <div className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-slate-200 bg-slate-50 px-3 py-3">
            <div className="min-w-0">
              <strong className="block text-[12px] text-slate-900">
                {loading
                  ? "설치 상태 확인 중…"
                  : !status?.supported
                    ? "이 환경에서는 지원되지 않음"
                    : status.installed
                      ? "오버레이 설치됨"
                      : "오버레이 미설치"}
              </strong>
              <span className="mt-0.5 block text-[10px] text-slate-500">
                {status?.installedVersion
                  ? `AppX ${status.installedVersion} · 자동 업데이트 ${status.updaterReady ? "준비됨" : "복구 필요"}`
                  : "Xbox Game Bar에서 League 위에 표시되는 선택형 오버레이입니다."}
              </span>
            </div>
            <Button
              variant="primary"
              disabled={loading || installing || status?.supported === false}
              onClick={() => void install()}
            >
              {installing
                ? "설치/복구 중…"
                : status?.installed
                  ? "복구/재설치"
                  : "오버레이 설치"}
            </Button>
          </div>
          <p className="text-[10px] leading-relaxed text-slate-500">
            최신 서명 manifest와 SHA256을 검증한 뒤 자동 설치합니다. 최초
            설치에서는 인증서와 localhost 예외 등록 때문에 Windows UAC 승인이 한
            번 뜰 수 있습니다.
          </p>
          {error && <p className="text-[10px] text-rose-600">{error}</p>}
        </div>
      </Card>

      <Card title="Win+G 사용 가이드">
        <div className="space-y-2">
          <GuideStep number={1} title="Xbox Game Bar 열기">
            게임 안에서 <Key>Win</Key> <span className="px-0.5">+</span>{" "}
            <Key>G</Key> 를 누릅니다. 처음 설치 직후라면 한 번 닫았다가 다시
            열면 새 위젯이 확실하게 로드됩니다.
          </GuideStep>
          <GuideStep number={2} title="유미지지 오버레이 열기">
            Game Bar 상단의 <strong>위젯 메뉴</strong>를 열고{" "}
            <strong>유미지지 오버레이</strong>를 선택합니다.
          </GuideStep>
          <GuideStep number={3} title="게임 중에도 보이게 고정">
            오버레이 위젯의 <strong>핀</strong>을 눌러 고정합니다. Game Bar를
            닫아도 핀으로 고정한 오버레이는 게임 위에 남습니다.
          </GuideStep>
          <GuideStep number={4} title="위치와 크기 맞추기">
            <Key>Win</Key> <span className="px-0.5">+</span> <Key>G</Key>{" "}
            상태에서 원하는 위치로 옮기고 크기를 조절합니다. 유튜브·Discord 채팅
            등 각 패널도 오버레이 설정에 맞춰 배치할 수 있습니다.
          </GuideStep>
        </div>
      </Card>

      <Card title="오버레이가 안 보일 때">
        <div className="grid gap-2 text-[10px] leading-relaxed text-slate-600 sm:grid-cols-2">
          <div className="rounded-lg border border-slate-200 bg-slate-50 px-3 py-2">
            <strong className="block text-[11px] text-slate-800">
              Win+G 자체가 안 열림
            </strong>
            Windows 설정에서 Xbox Game Bar가 활성화되어 있는지 확인하세요.
          </div>
          <div className="rounded-lg border border-slate-200 bg-slate-50 px-3 py-2">
            <strong className="block text-[11px] text-slate-800">
              위젯 목록에 없음
            </strong>
            위의 설치 상태를 새로고침하고, 필요하면 <strong>복구/재설치</strong>
            를 실행한 뒤 Game Bar를 다시 여세요.
          </div>
          <div className="rounded-lg border border-slate-200 bg-slate-50 px-3 py-2">
            <strong className="block text-[11px] text-slate-800">
              게임 중 사라짐
            </strong>
            Game Bar를 열어 오버레이 위젯의 핀이 켜져 있는지 확인하세요.
          </div>
          <div className="rounded-lg border border-slate-200 bg-slate-50 px-3 py-2">
            <strong className="block text-[11px] text-slate-800">
              연결 실패/내용이 비어 있음
            </strong>
            유미지지 데스크톱을 실행한 상태인지 확인하고, 계속되면 로그 탭의
            진단 정보를 확인하세요.
          </div>
        </div>
      </Card>
    </div>
  );
}
