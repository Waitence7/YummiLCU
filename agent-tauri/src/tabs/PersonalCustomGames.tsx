import { useCallback, useEffect, useState } from "react";
import { personalCustomGames } from "../api/commands";
import type { CustomGameRecord } from "../state/custom-games";

export function PersonalCustomGames({discordId,active,onBack}:{discordId?:string|null;active:boolean;onBack:()=>void}) {
  const [rows,setRows]=useState<CustomGameRecord[]>([]);
  const [error,setError]=useState("");
  const [busy,setBusy]=useState(false);
  const [filter,setFilter]=useState("all");
  const refresh=useCallback(()=>{
    let cancelled=false;
    setBusy(true);setError("");
    personalCustomGames().then(data=>{if(!cancelled)setRows(data.matches);})
      .catch(e=>{if(!cancelled){setError(String(e));setRows([]);}})
      .finally(()=>{if(!cancelled)setBusy(false);});
    return ()=>{cancelled=true;};
  },[]);
  const [revision,setRevision]=useState(0);
  useEffect(()=>{if(active&&discordId)return refresh();},[active,discordId,refresh,revision]);
  const list=rows.filter(r=>filter==="all"||(filter==="rift"?r.mode==="CLASSIC":r.mode!=="CLASSIC"));
  return <section className="h-full overflow-y-auto bg-white p-4 text-sm text-slate-700">
    <header className="mb-4 flex flex-wrap items-center gap-2">
      <button className="match-button" onClick={onBack}>‹ 일반 전적</button>
      <h1 className="mr-auto text-lg font-semibold">내 커스텀 게임</h1>
      <button className="match-button" disabled={!discordId||busy} onClick={()=>setRevision(n=>n+1)}>새로고침</button>
    </header>
    <p className="mb-3 text-xs text-slate-500">Yummi 계정에 기록된 경기 · 최근 100개 · 롤 클라이언트 없이 조회 가능</p>
    {!discordId?<p>Yummi에 로그인해 주세요.</p>:<>
      <select className="mb-3 rounded border px-2 py-1" aria-label="게임 모드" value={filter} onChange={e=>setFilter(e.target.value)}>
        <option value="all">전체</option><option value="rift">소환사의 협곡</option><option value="aram">칼바람 / 증바람</option>
      </select>
      {busy&&<p>기록을 불러오는 중…</p>}
      {error&&<p role="alert" className="text-red-600">{error}</p>}
      {!busy&&!error&&!list.length&&<p>저장된 커스텀 경기가 없습니다.</p>}
      <div className="space-y-2">{list.map(r=><article key={r.gameId} className="rounded-lg border border-slate-200 p-3">
        <div className="flex items-center justify-between gap-2">
          <strong>{r.mode==="CLASSIC"?"소환사의 협곡":r.mode==="ARAM"?"칼바람":"증바람"} · {r.champion??"챔피언 미확인"}</strong>
          <span className={r.won===true?"text-blue-600":r.won===false?"text-red-600":"text-slate-500"}>{r.won===true?"승리":r.won===false?"패배":r.status==="ended"?"결과 미확인":"감지 기록"}</span>
        </div>
        <p className="mt-1">{r.kills??"—"} / {r.deaths??"—"} / {r.assists??"—"}{!r.kdaFinal?" (마지막 관측)":""}</p>
        <p className="mt-1 text-xs text-slate-500">{new Date(r.playedAt).toLocaleString("ko-KR")} · {r.replayCollected?"ROFL 수집 완료":"ROFL 미수집"} · {r.inviteCode?"내전 "+r.inviteCode:"개인 기록"}</p>
      </article>)}</div>
    </>}
  </section>;
}
