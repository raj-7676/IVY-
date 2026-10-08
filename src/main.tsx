import { Component, ErrorInfo, ReactNode, StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import App from './App.tsx';
import './index.css';
import { soundEngine } from './utils/audio';

interface Props {
  children: ReactNode;
}

interface State {
  hasError: boolean;
  error: Error | null;
}

class RootErrorBoundary extends Component<Props, State> {
  public state: State = {
    hasError: false,
    error: null,
  };

  public static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error };
  }

  public componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    console.error('Uncaught React Error:', error, errorInfo);
  }

  public render() {
    if (this.state.hasError) {
      return (
        <div className="flex flex-col items-center justify-center h-screen w-screen bg-[#0a080e] text-white p-6 select-none font-sans">
          <div className="max-w-md w-full bg-white/[0.05] border border-white/10 rounded-2xl p-6 text-center space-y-4 shadow-2xl">
            <h2 className="text-base font-semibold text-amber-400">Something went wrong</h2>
            <p className="text-xs text-white/60 font-mono break-words bg-black/40 p-3 rounded-lg border border-white/5">
              {this.state.error?.message || 'An unexpected error occurred.'}
            </p>
            <button
              onClick={() => {
                sessionStorage.clear();
                window.location.reload();
              }}
              className="px-4 py-2 text-xs font-medium rounded-xl bg-orange-500 hover:bg-orange-600 text-white transition-colors"
            >
              Restart Ivy
            </button>
          </div>
        </div>
      );
    }

    return this.props.children;
  }
}

// Absorb the AudioContext's cold-start cost (audio device init) now,
// before the launch intro's impact sound needs to fire on time.
soundEngine.warmUp();

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <RootErrorBoundary>
      <App />
    </RootErrorBoundary>
  </StrictMode>,
);
