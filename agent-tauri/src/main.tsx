import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { useMockBridge } from './api/commands';
import { App } from './App';
import './styles.css';

const root = document.getElementById('root');
if (!root) throw new Error('Missing #root');

// 목 모드(브라우저 프리뷰)에서는 실제 앱 창 크기(640×620) 프레임 안에 렌더링한다.
const app = useMockBridge ? (
  <div className="flex min-h-full flex-col items-center justify-center gap-3 bg-slate-100 p-6">
    <p className="text-[12px] text-slate-500">
      UI 프리뷰 — 목 데이터로 동작합니다 (실제 롤 클라이언트·서버 연결 없음)
    </p>
    <div className="h-[620px] w-[640px] shrink-0 overflow-hidden rounded-xl border border-slate-300 shadow-2xl">
      <App />
    </div>
  </div>
) : (
  <App />
);

createRoot(root).render(<StrictMode>{app}</StrictMode>);
