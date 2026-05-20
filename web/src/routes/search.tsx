import { createFileRoute } from '@tanstack/react-router';
import { useEffect, useRef, useState } from 'react';

import { Icon, KindBadge, ProjBadge, SessionPill } from '@/components/atoms';
import { MOCK } from '@/lib/mock-data';

export const Route = createFileRoute('/search')({
  component: SearchPage,
});

type Chip = { k: string; v: string };

const CHIP_KEYS = ['kind', 'project', 'session', 'urgency'] as const;
const VALUES_FOR: Record<(typeof CHIP_KEYS)[number], string[]> = {
  kind: [
    'learning',
    'accomplishment',
    'decision',
    'blocker',
    'todo',
    'user_action',
    'goal_phase',
    'fact',
  ],
  project: ['contextnest', 'z-insight', 'ratchet', 'scratch-llm'],
  session: ['cc-7f3a2e91', 'cc-a812bc40', 'cc-d4f0e8b2'],
  urgency: ['now', 'soon', 'later'],
};

function SearchPage() {
  const [q, setQ] = useState('mpsc back-pressure');
  const [chips, setChips] = useState<Chip[]>([{ k: 'kind', v: 'learning' }]);
  const [focused, setFocused] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const removeChip = (i: number) => setChips((c) => c.filter((_, j) => j !== i));
  const addChip = (k: string, v: string) => setChips((c) => [...c, { k, v }]);

  const all = MOCK.searchResults;
  const results =
    q.length === 0
      ? []
      : all.filter((r) => {
          for (const c of chips) {
            if (c.k === 'kind' && r.kind !== c.v) return false;
            if (c.k === 'project' && r.project !== c.v) return false;
            if (c.k === 'session' && r.sessionId !== c.v) return false;
          }
          return true;
        });

  return (
    <div>
      <div className="page-header">
        <div>
          <h1 className="page-title">Search</h1>
          <div className="page-sub">
            Semantic + metadata search across every memory · scoped to one session or cross-session
          </div>
        </div>
        <div className="page-actions">
          <span className="mono dim" style={{ fontSize: 11 }}>
            cmd+/ to focus
          </span>
        </div>
      </div>

      <div className="search-input">
        <Icon.Search className="icon" style={{ width: 16, height: 16 }} />
        <input
          ref={inputRef}
          value={q}
          onChange={(e) => setQ(e.target.value)}
          placeholder="search memories — try 'awaiting decisions', 'embedding choices', 'migration plan'…"
        />
        {q && (
          <button className="btn btn-ghost sm" onClick={() => setQ('')} type="button">
            <Icon.X />
          </button>
        )}
      </div>

      <div className="filter-bar" style={{ paddingTop: 12, paddingBottom: 14 }}>
        {chips.map((c, i) => (
          <span className="chip active" key={i}>
            <span style={{ color: 'var(--ink-faint)' }}>{c.k}:</span>
            {c.v}
            <span className="x" onClick={() => removeChip(i)}>
              <Icon.X />
            </span>
          </span>
        ))}
        <ChipAdder onAdd={addChip} />
        <div className="grow" />
        <span className="mono dim" style={{ fontSize: 11 }}>
          {q ? `${results.length} hits · ${chips.length} filters` : 'type to search'}
        </span>
      </div>

      {q === '' && (
        <div className="filter-bar" style={{ paddingTop: 0 }}>
          <span className="mono dim" style={{ fontSize: 11, marginRight: 8 }}>
            quick searches:
          </span>
          <span className="chip" onClick={() => setQ('recent learnings')}>
            recent learnings
          </span>
          <span className="chip" onClick={() => setQ('open decisions')}>
            open decisions
          </span>
          <span className="chip" onClick={() => setQ('blockers')}>
            blockers across all projects
          </span>
          <span className="chip" onClick={() => setQ('user actions today')}>
            user actions today
          </span>
        </div>
      )}

      <div className="card" style={{ padding: 0 }}>
        {q === '' ? (
          <div className="empty">
            <Icon.Search
              style={{ width: 36, height: 36, color: 'var(--ink-faint)', opacity: 0.6 }}
            />
            <div className="empty-title">Search the substrate</div>
            <div className="empty-body">
              Type a query above, or click a quick search. Add chips to constrain by kind, project,
              or session.
            </div>
          </div>
        ) : results.length === 0 ? (
          <div className="empty">
            <div className="empty-title">No memories match</div>
            <div className="empty-body">Try removing a filter or broadening the query.</div>
          </div>
        ) : (
          results.map((r, i) => (
            <div
              key={i}
              className={`search-result${focused === i ? ' focused' : ''}`}
              onMouseEnter={() => setFocused(i)}
            >
              <div>
                <div className="meta-row">
                  <KindBadge kind={r.kind} />
                  <SessionPill id={r.sessionId} />
                  <ProjBadge p={r.project} />
                  <span>· {r.stored}</span>
                </div>
                <div className="snippet" dangerouslySetInnerHTML={{ __html: r.snippet }} />
              </div>
              <div className="sim">
                <div className="mono" style={{ fontSize: 10.5 }}>
                  sim · {r.similarity.toFixed(2)}
                </div>
                <div className="sim-bar">
                  <span style={{ width: `${r.similarity * 100}%` }} />
                </div>
                <div
                  className="mono"
                  style={{ fontSize: 9.5, color: 'var(--ink-faint)', marginTop: 2 }}
                >
                  tf-idf · 256d
                </div>
              </div>
            </div>
          ))
        )}
      </div>

      <div className="note-banner" style={{ marginTop: 14 }}>
        <span className="dot" style={{ background: 'var(--urg-soon)' }} />
        <span>
          Substrate is on the hash-based TF-IDF embedder by default — similarity scores are
          uniformly high and not yet a reliable rank signal. Swap to Ollama or OpenAI from{' '}
          <a style={{ color: 'var(--accent)' }} href="#" onClick={(e) => e.preventDefault()}>
            Substrate · embedding
          </a>
          .
        </span>
      </div>
    </div>
  );
}

function ChipAdder({ onAdd }: { onAdd: (k: string, v: string) => void }) {
  const [open, setOpen] = useState(false);
  const [key, setKey] = useState<(typeof CHIP_KEYS)[number] | null>(null);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const fn = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false);
        setKey(null);
      }
    };
    document.addEventListener('click', fn);
    return () => document.removeEventListener('click', fn);
  }, []);

  return (
    <div ref={ref} style={{ position: 'relative' }}>
      <span className="chip add" onClick={() => setOpen((o) => !o)}>
        <Icon.Plus /> filter
      </span>
      {open && (
        <div
          style={{
            position: 'absolute',
            left: 0,
            top: 'calc(100% + 4px)',
            background: 'var(--surface-2)',
            border: '1px solid var(--border)',
            borderRadius: 8,
            padding: 4,
            minWidth: 180,
            zIndex: 10,
            boxShadow: 'var(--shadow-pop)',
          }}
        >
          {!key ? (
            CHIP_KEYS.map((k) => (
              <div
                key={k}
                onClick={() => setKey(k)}
                style={{
                  padding: '6px 10px',
                  borderRadius: 4,
                  cursor: 'pointer',
                  fontFamily: 'var(--font-mono)',
                  fontSize: 12,
                  color: 'var(--ink-muted)',
                }}
              >
                {k}
              </div>
            ))
          ) : (
            <>
              <div
                style={{
                  padding: '4px 10px',
                  fontFamily: 'var(--font-mono)',
                  fontSize: 10,
                  color: 'var(--ink-faint)',
                  textTransform: 'uppercase',
                }}
              >
                {key} ·
              </div>
              {VALUES_FOR[key].map((v) => (
                <div
                  key={v}
                  onClick={() => {
                    onAdd(key, v);
                    setOpen(false);
                    setKey(null);
                  }}
                  style={{
                    padding: '5px 10px',
                    borderRadius: 4,
                    cursor: 'pointer',
                    fontFamily: 'var(--font-mono)',
                    fontSize: 12,
                    color: 'var(--ink)',
                  }}
                >
                  {v}
                </div>
              ))}
            </>
          )}
        </div>
      )}
    </div>
  );
}
