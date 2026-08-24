import { useEffect, useRef, useState } from 'react';

import { Button } from '../components/ui';

export function LogsTab({
  logs,
  onGetDiagnostics,
  onExportDiagnostics,
}: {
  logs: string[];
  onGetDiagnostics(): Promise<string>;
  onExportDiagnostics(): Promise<string>;
}) {
  const scroller = useRef<HTMLPreElement>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState<'copy' | 'export' | null>(null);

  useEffect(() => {
    const element = scroller.current;
    if (element) element.scrollTop = element.scrollHeight;
  }, [logs]);

  async function copyDiagnostics() {
    setBusy('copy');
    setNotice(null);
    try {
      const bundle = await onGetDiagnostics();
      await navigator.clipboard.writeText(bundle);
      setNotice('진단 정보를 클립보드에 복사했습니다.');
    } catch (error) {
      setNotice(`진단 정보 복사 실패: ${String(error)}`);
    } finally {
      setBusy(null);
    }
  }

  async function exportDiagnostics() {
    setBusy('export');
    setNotice(null);
    try {
      const path = await onExportDiagnostics();
      setNotice(`진단 파일 저장됨: ${path}`);
    } catch (error) {
      setNotice(`진단 파일 저장 실패: ${String(error)}`);
    } finally {
      setBusy(null);
    }
  }

  return (
    <div className="flex h-full flex-col rounded-xl border border-slate-200 bg-white">
      <div className="flex items-center justify-between gap-3 border-b border-slate-200 px-3 py-2">
        <div className="min-w-0">
          <span className="text-[12px] font-semibold text-slate-700">로그</span>
          <span className="ml-2 text-[11px] text-slate-500">
            {logs.length}줄 · 최근 2,000줄 유지
          </span>
        </div>
        <div className="flex shrink-0 items-center gap-1.5">
          <Button
            variant="ghost"
            disabled={busy !== null}
            onClick={() => void copyDiagnostics()}
          >
            {busy === 'copy' ? '복사 중…' : '진단 복사'}
          </Button>
          <Button disabled={busy !== null} onClick={() => void exportDiagnostics()}>
            {busy === 'export' ? '저장 중…' : '파일 저장'}
          </Button>
        </div>
      </div>
      {notice && (
        <div className="border-b border-slate-100 bg-slate-50 px-3 py-1.5 text-[10px] text-slate-600">
          {notice}
        </div>
      )}
      <pre
        ref={scroller}
        className="min-h-0 flex-1 select-text overflow-auto p-3 font-mono text-[11px] leading-relaxed whitespace-pre-wrap text-slate-600"
      >
        {logs.length > 0 ? logs.join('\n') : '아직 로그가 없습니다.'}
      </pre>
    </div>
  );
}
