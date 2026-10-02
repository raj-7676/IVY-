import React, { useEffect, useRef } from 'react';

interface Particle {
  x: number;
  y: number;
  z: number; // Simulated depth: 0.3 (far/small/dim) to 1.6 (near/large/glowing)
  vx: number;
  vy: number;
  size: number;
  baseAlpha: number;
  alphaPhase: number;
  alphaSpeed: number;
  colorType: 'white' | 'ember';
  wobbleSpeed: number;
  wobblePhase: number;
}

interface StaticSparkle {
  x: number;
  y: number;
  life: number;
  maxLife: number;
  alpha: number;
  size: number;
}

export const AtmosphericDust: React.FC = () => {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d', { alpha: true });
    if (!ctx) return;

    let animId: number;
    let width = (canvas.width = window.innerWidth);
    let height = (canvas.height = window.innerHeight);

    // Track mouse for subtle air displacement
    let mouseX = width / 2;
    let mouseY = height / 2;
    let prevMouseX = mouseX;
    let prevMouseY = mouseY;
    let mouseSpeed = 0;

    const handleMouseMove = (e: MouseEvent) => {
      const dx = e.clientX - prevMouseX;
      const dy = e.clientY - prevMouseY;
      mouseSpeed = Math.min(12, Math.sqrt(dx * dx + dy * dy));
      mouseX = e.clientX;
      mouseY = e.clientY;
      prevMouseX = mouseX;
      prevMouseY = mouseY;
    };

    window.addEventListener('mousemove', handleMouseMove, { passive: true });

    // Handle high-DPI screens and resizing
    const handleResize = () => {
      if (!canvas) return;
      const dpr = Math.min(window.devicePixelRatio || 1, 2);
      width = window.innerWidth;
      height = window.innerHeight;
      canvas.width = width * dpr;
      canvas.height = height * dpr;
      canvas.style.width = `${width}px`;
      canvas.style.height = `${height}px`;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    };

    handleResize();
    window.addEventListener('resize', handleResize);

    // Generate balanced density based on screen dimensions (~45 to 70 particles)
    const particleCount = Math.max(35, Math.min(75, Math.floor((width * height) / 24000)));
    const particles: Particle[] = [];

    const spawnParticle = (startY?: number): Particle => {
      const z = 0.3 + Math.random() * 1.3;
      const isEmber = Math.random() < 0.22; // 22% warm ember hue echoing IVY brand accents
      return {
        x: Math.random() * width,
        y: startY !== undefined ? startY : Math.random() * height,
        z,
        vx: (Math.random() - 0.5) * 0.22 * z,
        vy: -(0.12 + Math.random() * 0.28) * z, // Gentle upward convective drift
        size: (0.75 + Math.random() * 1.3) * z,
        baseAlpha: (0.12 + Math.random() * 0.35) * (z > 1.2 ? 0.85 : 1),
        alphaPhase: Math.random() * Math.PI * 2,
        alphaSpeed: 0.008 + Math.random() * 0.02,
        colorType: isEmber ? 'ember' : 'white',
        wobbleSpeed: 0.004 + Math.random() * 0.008,
        wobblePhase: Math.random() * Math.PI * 2,
      };
    };

    for (let i = 0; i < particleCount; i++) {
      particles.push(spawnParticle());
    }

    // Ephemeral digital/film static sparkles
    const sparkles: StaticSparkle[] = [];
    const maxSparkles = 8;

    let time = 0;

    const render = () => {
      time += 1;
      ctx.clearRect(0, 0, width, height);

      // Decelerate mouse air disturbance
      mouseSpeed *= 0.92;

      // 1. RENDER & UPDATE FLOATING DUST MOTES
      for (let i = 0; i < particles.length; i++) {
        const p = particles[i];

        // Organic sinusoidal air currents
        const airCurrentX = Math.sin(time * p.wobbleSpeed + p.wobblePhase) * 0.25;
        const airCurrentY = Math.cos(time * p.wobbleSpeed * 0.8 + p.wobblePhase) * 0.1;

        // Interactive mouse wind wake
        const dx = p.x - mouseX;
        const dy = p.y - mouseY;
        const distSq = dx * dx + dy * dy;
        const effectRadius = 140;

        if (distSq < effectRadius * effectRadius && mouseSpeed > 1) {
          const dist = Math.sqrt(distSq);
          const force = (1 - dist / effectRadius) * 0.6;
          p.x += (dx / (dist || 1)) * force * (mouseSpeed * 0.15);
          p.y += (dy / (dist || 1)) * force * (mouseSpeed * 0.15);
        }

        p.x += p.vx + airCurrentX;
        p.y += p.vy + airCurrentY;

        // Wrap around viewport edges smoothly
        if (p.y < -20) {
          p.y = height + 15;
          p.x = Math.random() * width;
        } else if (p.y > height + 20) {
          p.y = -15;
          p.x = Math.random() * width;
        }

        if (p.x < -20) {
          p.x = width + 15;
        } else if (p.x > width + 20) {
          p.x = -15;
        }

        // Oscillating light reflectivity / twinkle
        p.alphaPhase += p.alphaSpeed;
        const alphaTwinkle = 0.75 + Math.sin(p.alphaPhase) * 0.25;
        const currentAlpha = Math.max(0.04, Math.min(0.65, p.baseAlpha * alphaTwinkle));

        ctx.beginPath();
        ctx.arc(p.x, p.y, p.size, 0, Math.PI * 2);

        if (p.colorType === 'ember') {
          ctx.fillStyle = `rgba(255, 128, 48, ${currentAlpha.toFixed(3)})`;
          if (p.z > 1.0) {
            ctx.shadowBlur = 4 * p.z;
            ctx.shadowColor = 'rgba(255, 85, 0, 0.4)';
          } else {
            ctx.shadowBlur = 0;
          }
        } else {
          ctx.fillStyle = `rgba(235, 242, 255, ${currentAlpha.toFixed(3)})`;
          if (p.z > 1.2) {
            ctx.shadowBlur = 3 * p.z;
            ctx.shadowColor = 'rgba(255, 255, 255, 0.35)';
          } else {
            ctx.shadowBlur = 0;
          }
        }

        ctx.fill();
      }

      // Reset shadow for subsequent passes
      ctx.shadowBlur = 0;

      // 2. OCCASIONAL MICRO-STATIC / CRT GRAIN SPARKLES
      if (Math.random() < 0.14 && sparkles.length < maxSparkles) {
        sparkles.push({
          x: Math.random() * width,
          y: Math.random() * height,
          life: 0,
          maxLife: 2 + Math.floor(Math.random() * 4), // 2-5 frames duration
          alpha: 0.08 + Math.random() * 0.18,
          size: Math.random() < 0.7 ? 1.0 : 1.5,
        });
      }

      for (let i = sparkles.length - 1; i >= 0; i--) {
        const s = sparkles[i];
        s.life += 1;
        if (s.life >= s.maxLife) {
          sparkles.splice(i, 1);
          continue;
        }

        const progress = s.life / s.maxLife;
        const sparkleAlpha = s.alpha * (1 - Math.abs(progress - 0.5) * 2);

        ctx.fillStyle = Math.random() < 0.3
          ? `rgba(255, 136, 68, ${sparkleAlpha.toFixed(3)})`
          : `rgba(240, 246, 255, ${sparkleAlpha.toFixed(3)})`;

        ctx.fillRect(s.x, s.y, s.size, s.size);
      }

      animId = requestAnimationFrame(render);
    };

    animId = requestAnimationFrame(render);

    return () => {
      cancelAnimationFrame(animId);
      window.removeEventListener('mousemove', handleMouseMove);
      window.removeEventListener('resize', handleResize);
    };
  }, []);

  return (
    <canvas
      id="atmospheric-dust-canvas"
      ref={canvasRef}
      className="fixed inset-0 pointer-events-none z-[60] opacity-85 transition-opacity duration-1000"
      style={{ mixBlendMode: 'screen' }}
      aria-hidden="true"
    />
  );
};
