import * as api from '../api/commands';
import appIcon from '../../src-tauri/icons/icon.ico';

export function TitleBar() {
  return (
    <div
      className="flex h-10 shrink-0 cursor-move items-center border-b border-white/10 bg-[#2b2b2b] text-slate-100"
      onMouseDown={(event) => {
        if (event.button !== 0 || (event.target as HTMLElement).closest('button')) return;
        event.preventDefault();
        void api.startMainWindowDrag().catch((error) => {
          console.warn('[window] 창 드래그를 시작하지 못했습니다.', error);
        });
      }}
    >
      <div className="flex min-w-0 flex-1 items-center gap-2.5 px-3">
        <img src={appIcon} alt="" className="size-5 shrink-0" draggable={false} />
        <span className="truncate text-[13px] font-medium tracking-[0.01em]">
          Yummi LCU Agent
        </span>
      </div>
      <div className="flex h-full shrink-0" aria-label="창 제어">
        <button
          type="button"
          aria-label="최소화"
          title="최소화"
          className="grid h-full w-11 place-items-center text-slate-300 transition-colors hover:bg-white/10 hover:text-white"
          onClick={() => void api.minimizeMainWindow()}
        >
          <span className="block h-px w-3.5 bg-current" />
        </button>
        <button
          type="button"
          aria-label="트레이로 보내기"
          title="트레이로 보내기"
          className="grid h-full w-11 place-items-center text-slate-300 transition-colors hover:bg-[#c42b1c] hover:text-white"
          onClick={() => void api.requestTrayHide()}
        >
          <span className="relative block size-3.5 before:absolute before:top-1.5 before:left-[-1px] before:h-px before:w-4 before:rotate-45 before:bg-current after:absolute after:top-1.5 after:left-[-1px] after:h-px after:w-4 after:-rotate-45 after:bg-current" />
        </button>
      </div>
    </div>
  );
}
