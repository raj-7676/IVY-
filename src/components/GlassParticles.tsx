import React, { useEffect, useRef } from 'react';

interface GlassParticlesProps {
  /** Accent as "r, g, b" — e.g. "255, 107, 0". */
  accentRgb: string;
  count?: number;
}

interface Particle {
  x: number;
  y: number;
  vx: number;
  vy: number;
  size: number;
  baseAlpha: number;
  alpha: number;
  twinkleSpeed: number;
  phase: number;
  type: 'circle' | 'diamond' | 'shard';
  rotation: number;
  rotSpeed: number;
}

// Drifting crystalline glass motes behind the app chrome — the ambient layer
// that makes the window read as glass rather than a flat dark rectangle.
export const GlassParticles: React.FC<GlassParticlesProps> = ({ accentRgb, count = 26 }) => {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    let width = (canvas.width = canvas.parentElement?.clientWidth || 1120);
    let height = (canvas.height = canvas.parentElement?.clientHeight || 720);

    const resizeObserver = new ResizeObserver(() => {
      if (!canvas.parentElement) return;
      width = canvas.width = canvas.parentElement.clientWidth;
      height = canvas.height = canvas.parentElement.clientHeight;
    });
    if (canvas.parentElement) resizeObserver.observe(canvas.parentElement);

    const types: Particle['type'][] = ['circle', 'diamond', 'shard', 'circle'];
    const particles: Particle[] = Array.from({ length: count }, () => ({
      x: Math.random() * width,
      y: Math.random() * height,
      vx: (Math.random() - 0.5) * 0.35,
      vy: -0.2 - Math.random() * 0.45,
      size: 1.2 + Math.random() * 2.8,
      baseAlpha: 0.18 + Math.random() * 0.32,
      alpha: 0.2,
      twinkleSpeed: 0.02 + Math.random() * 0.035,
      phase: Math.random() * Math.PI * 2,
      type: types[Math.floor(Math.random() * types.length)],
      rotation: Math.random() * Math.PI * 2,
      rotSpeed: (Math.random() - 0.5) * 0.02,
    }));

    let animFrame: number;
    let lastTime = performance.now();

    const render = (time: number) => {
      const dt = Math.min(32, time - lastTime) / 16;
      lastTime = time;
      ctx.clearRect(0, 0, width, height);

      for (const p of particles) {
        p.x += p.vx * dt;
        p.y += p.vy * dt;
        p.rotation += p.rotSpeed * dt;
        p.phase += p.twinkleSpeed * dt;
        p.alpha = Math.max(0.05, p.baseAlpha + Math.sin(p.phase) * 0.18);

        if (p.y < -10) {
          p.y = height + 10;
          p.x = Math.random() * width;
        }
        if (p.x < -10) p.x = width + 10;
        else if (p.x > width + 10) p.x = -10;

        const a = Math.max(0, Math.min(0.85, p.alpha));
        const size = Math.max(0.1, p.size);

        ctx.save();
        ctx.translate(p.x, p.y);
        ctx.rotate(p.rotation);

        if (p.type === 'circle') {
          const r = size * 2.2;
          const grad = ctx.createRadialGradient(0, 0, 0, 0, 0, r);
          grad.addColorStop(0, `rgba(255, 255, 255, ${a * 1.2})`);
          grad.addColorStop(0.35, `rgba(${accentRgb}, ${a})`);
          grad.addColorStop(1, `rgba(${accentRgb}, 0)`);
          ctx.fillStyle = grad;
          ctx.beginPath();
          ctx.arc(0, 0, r, 0, Math.PI * 2);
          ctx.fill();
        } else if (p.type === 'diamond') {
          ctx.fillStyle = `rgba(255, 255, 255, ${a * 1.1})`;
          ctx.strokeStyle = `rgba(${accentRgb}, ${a * 0.9})`;
          ctx.lineWidth = 0.6;
          ctx.beginPath();
          ctx.moveTo(0, -size * 1.4);
          ctx.lineTo(size * 0.9, 0);
          ctx.lineTo(0, size * 1.4);
          ctx.lineTo(-size * 0.9, 0);
          ctx.closePath();
          ctx.fill();
          ctx.stroke();
        } else {
          ctx.fillStyle = `rgba(${accentRgb}, ${a * 0.8})`;
          ctx.beginPath();
          ctx.moveTo(0, -size * 1.8);
          ctx.lineTo(size * 0.4, 0);
          ctx.lineTo(0, size * 1.8);
          ctx.lineTo(-size * 0.4, 0);
          ctx.closePath();
          ctx.fill();
          ctx.fillStyle = `rgba(255, 255, 255, ${a * 1.4})`;
          ctx.beginPath();
          ctx.arc(0, 0, size * 0.45, 0, Math.PI * 2);
          ctx.fill();
        }

        ctx.restore();
      }

      animFrame = requestAnimationFrame(render);
    };

    animFrame = requestAnimationFrame(render);
    return () => {
      cancelAnimationFrame(animFrame);
      resizeObserver.disconnect();
    };
  }, [count, accentRgb]);

  return (
    <canvas
      ref={canvasRef}
      aria-hidden="true"
      className="pointer-events-none absolute inset-0 z-[1] w-full h-full"
    />
  );
};
