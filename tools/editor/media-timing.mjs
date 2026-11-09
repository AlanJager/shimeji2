// Quantize cumulative boundaries, retaining the rounded total whenever the
// mandatory one-tick minimum permits it. All engine durations remain integers.
export function quantizeDurations(durations) {
  if (!Array.isArray(durations) || !durations.length || durations.length > 256) throw new Error('动画需要包含 1–256 帧。');
  for (const ms of durations) if (!Number.isFinite(ms) || ms < 0 || ms > 60000) throw new Error('每帧原始时长需要在 0–60000 ms 之间。');
  const originalMs = durations.reduce((sum, ms) => sum + ms, 0);
  const target = Math.max(durations.length, Math.round(originalMs / 40));
  let cumulative = 0, previous = 0;
  const ticks = durations.map((ms, index) => {
    cumulative += ms;
    const remaining = durations.length - index - 1;
    const boundary = Math.max(previous + 1, target - remaining * 1500,
      Math.min(previous + 1500, target - remaining, Math.round(cumulative / 40)));
    const duration = boundary - previous; previous = boundary; return duration;
  });
  return {ticks,originalMs,adjustedMs:target * 40,changed:ticks.some((tick,index) => tick * 40 !== durations[index])};
}

export function timedFrameAt(frames, tick, loop = true) {
  if (!frames.length) throw new Error('预览没有动画帧。');
  const total = frames.reduce((sum, frame) => sum + frame.ticks, 0);
  let at = loop ? ((tick % total) + total) % total : Math.max(0, Math.min(total - 1, tick));
  for (let index = 0; index < frames.length; index++) {
    if (at < frames[index].ticks) return index;
    at -= frames[index].ticks;
  }
  return frames.length - 1;
}
