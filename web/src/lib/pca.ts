// Pure-JS PCA via power iteration. No external dependency.
//
// The /field viz uses this to project 256-d embeddings into 2-d for
// the canvas. PCA is the right tool here because we want a LINEAR
// projection that preserves global structure (clusters at one end,
// outliers at the other) over local structure (which UMAP would
// optimize). For this audience — engineers reading the field at a
// glance — global structure is more useful: "where in concept space
// does this fragment sit relative to everything else?"
//
// Algorithm:
//   1. Center the data by subtracting per-dim means.
//   2. Power iteration on the implicit covariance matrix to find PC1.
//      (We never materialize the 256×256 covariance matrix — that'd
//      cost 65k floats. Instead each iteration is two N-d sweeps:
//      project all vectors onto the current PC, then accumulate the
//      weighted sum back. Memory stays O(D + N).)
//   3. Deflate against PC1 and repeat for PC2.
//   4. Project every point onto (PC1, PC2). Return the 2-d coords plus
//      the eigenvalues so callers can show "variance explained" if
//      they want.
//
// Numerical stability: 60 iterations is more than enough for 256-d
// data — power iteration converges geometrically with rate
// (λ₂/λ₁). On real ContextNest embeddings (TF-IDF-based) the ratio
// is usually ~0.3 so 60 iters drops error by ~10⁻³⁰.

export type Pca2dResult = {
  /** Projected 2D coordinates, same order as input. */
  coords: Array<{ x: number; y: number }>;
  /** Variance captured by PC1 + PC2 / total variance. 0..1. */
  varianceRatio: number;
};

const ITERS = 60;

function dot(a: number[], b: number[]): number {
  let s = 0;
  for (let i = 0; i < a.length; i++) s += a[i] * b[i];
  return s;
}

function norm(v: number[]): number {
  let s = 0;
  for (let i = 0; i < v.length; i++) s += v[i] * v[i];
  return Math.sqrt(s);
}

function scaleInPlace(v: number[], s: number): void {
  for (let i = 0; i < v.length; i++) v[i] *= s;
}

function addScaledInPlace(target: number[], src: number[], s: number): void {
  for (let i = 0; i < target.length; i++) target[i] += src[i] * s;
}

/**
 * Power-iterate to find the dominant eigenvector of the data's covariance
 * matrix, without materializing the matrix.
 *
 * Each iteration:
 *   1. proj[i] = data[i] · v       (N projections)
 *   2. v' = Σ_i proj[i] · data[i]  (N D-dim adds)
 *   3. v = v' / ||v'||
 *
 * On convergence, v is PC1 and ||v'|| approximates the corresponding
 * eigenvalue × N (we divide out N at the end).
 */
function powerIteration(data: number[][], seed: number[]): { pc: number[]; eigenvalue: number } {
  const dim = seed.length;
  let v = seed.slice();
  let lastEigen = 0;
  for (let iter = 0; iter < ITERS; iter++) {
    const next = new Array<number>(dim).fill(0);
    for (let i = 0; i < data.length; i++) {
      const proj = dot(data[i], v);
      addScaledInPlace(next, data[i], proj);
    }
    const n = norm(next);
    if (n < 1e-12) break;
    scaleInPlace(next, 1 / n);
    v = next;
    lastEigen = n / Math.max(1, data.length);
  }
  return { pc: v, eigenvalue: lastEigen };
}

function deflate(data: number[][], pc: number[]): number[][] {
  // Remove the PC1 component from every vector: x' = x - (x · pc) pc
  return data.map((row) => {
    const proj = dot(row, pc);
    return row.map((val, i) => val - proj * pc[i]);
  });
}

/**
 * Project N rows of D-dimensional embeddings into 2-D.
 *
 * Returns the projected coordinates in the original order, plus the
 * fraction of total variance captured by PC1 + PC2 (a quality
 * indicator the caller can show: "98% variance — these 2 axes hold
 * almost everything").
 *
 * Empty / degenerate input is handled gracefully — N < 2 returns zero
 * coordinates with `varianceRatio = 0`.
 */
export function pca2d(rows: number[][]): Pca2dResult {
  if (rows.length < 2 || rows[0].length === 0) {
    return {
      coords: rows.map(() => ({ x: 0, y: 0 })),
      varianceRatio: 0,
    };
  }

  const n = rows.length;
  const dim = rows[0].length;

  // Step 1: center.
  const mean = new Array<number>(dim).fill(0);
  for (const r of rows) {
    for (let i = 0; i < dim; i++) mean[i] += r[i];
  }
  for (let i = 0; i < dim; i++) mean[i] /= n;
  const centered = rows.map((r) => r.map((v, i) => v - mean[i]));

  // Total variance (sum of squared deviations / N). Used to compute the
  // explained-variance ratio at the end.
  let totalVar = 0;
  for (const r of centered) totalVar += dot(r, r);
  totalVar /= n;

  // Deterministic seed for PC1: alternating ±1/√D direction — gives
  // stable iteration across refreshes without needing a PRNG. Any
  // nonzero seed works mathematically; deterministic is required for
  // visual stability.
  const seed1 = new Array<number>(dim)
    .fill(0)
    .map((_, i) => (i % 2 === 0 ? 1 : -1) / Math.sqrt(dim));
  const { pc: pc1, eigenvalue: e1 } = powerIteration(centered, seed1);

  // Deflate and find PC2.
  const deflated = deflate(centered, pc1);
  // Seed PC2 with a different alternating pattern (every-third sign flip)
  // to avoid landing in the same eigenspace as PC1.
  const seed2 = new Array<number>(dim)
    .fill(0)
    .map((_, i) => (i % 3 === 0 ? -1 : 1) / Math.sqrt(dim));
  const { pc: pc2, eigenvalue: e2 } = powerIteration(deflated, seed2);

  // Project. We use the CENTERED rows (not deflated) so PC2 axis still
  // reflects PC2 variance — deflation was only used to find pc2 itself.
  const coords = centered.map((r) => ({
    x: dot(r, pc1),
    y: dot(r, pc2),
  }));

  const varianceRatio =
    totalVar > 1e-12 ? Math.min(1, (e1 + e2) / totalVar) : 0;

  return { coords, varianceRatio };
}

/**
 * Cosine similarity in [-1, 1]. Used for the hover-to-light-up-
 * nearest-neighbors interaction on the field. Returns 0 when either
 * vector has zero norm (defensive, shouldn't happen for normalized
 * embeddings).
 */
export function cosineSimilarity(a: number[], b: number[]): number {
  if (a.length !== b.length || a.length === 0) return 0;
  let dotAB = 0;
  let normA = 0;
  let normB = 0;
  for (let i = 0; i < a.length; i++) {
    dotAB += a[i] * b[i];
    normA += a[i] * a[i];
    normB += b[i] * b[i];
  }
  const denom = Math.sqrt(normA) * Math.sqrt(normB);
  return denom < 1e-12 ? 0 : dotAB / denom;
}
