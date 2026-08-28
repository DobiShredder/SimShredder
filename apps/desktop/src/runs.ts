export type RunReference =
  | { kind: "quick"; jobId: number }
  | { kind: "topGear"; sessionId: string };

export const runKey = (run: RunReference) => run.kind === "quick"
  ? `quick-${run.jobId}`
  : `top-gear-${run.sessionId}`;

export const sameRun = (left: RunReference | null, right: RunReference) => Boolean(
  left && left.kind === right.kind && (
    left.kind === "quick"
      ? left.jobId === (right as Extract<RunReference, { kind: "quick" }>).jobId
      : left.sessionId === (right as Extract<RunReference, { kind: "topGear" }>).sessionId
  ),
);
