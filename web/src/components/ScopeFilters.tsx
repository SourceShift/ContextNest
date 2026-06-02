import { type KeyboardEvent, useEffect, useMemo, useRef, useState } from 'react';

import { basenameOf, type ProjectOption } from '@/lib/scope';
import type { SessionListItem } from '@/lib/types';

type ScopePickerOption = {
  value: string;
  label: string;
  meta: string;
  searchText: string;
};

type ScopePickerProps = {
  label: string;
  value: string | undefined;
  options: ScopePickerOption[];
  placeholder: string;
  disabled?: boolean;
  disabledPlaceholder?: string;
  onChange: (value: string | undefined) => void;
  allowEmptyLabel?: string;
  minWidth?: number;
};

function ScopePicker({
  label,
  value,
  options,
  placeholder,
  disabled,
  disabledPlaceholder,
  onChange,
  allowEmptyLabel,
  minWidth = 300,
}: ScopePickerProps) {
  const [query, setQuery] = useState('');
  const [open, setOpen] = useState(false);
  const [highlight, setHighlight] = useState(0);
  const wrapperRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  const selected = value ? options.find((o) => o.value === value) : undefined;
  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return options;
    return options.filter((o) => o.searchText.toLowerCase().includes(q));
  }, [options, query]);
  const activeHighlight = Math.min(highlight, Math.max(0, filtered.length - 1));

  useEffect(() => {
    if (!open || !listRef.current) return;
    const node = listRef.current.querySelector<HTMLDivElement>(
      `[data-scope-row="${activeHighlight}"]`,
    );
    node?.scrollIntoView({ block: 'nearest' });
  }, [activeHighlight, open]);

  useEffect(() => {
    const onDocClick = (e: MouseEvent) => {
      if (!wrapperRef.current?.contains(e.target as Node)) {
        setOpen(false);
        setQuery('');
      }
    };
    document.addEventListener('mousedown', onDocClick);
    return () => document.removeEventListener('mousedown', onDocClick);
  }, []);

  const commit = (next: string | undefined) => {
    onChange(next);
    setOpen(false);
    setQuery('');
    inputRef.current?.blur();
  };

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      if (!open) setOpen(true);
      setHighlight((h) => Math.min(filtered.length - 1, h + 1));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setHighlight((h) => Math.max(0, h - 1));
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if (filtered.length > 0) commit(filtered[activeHighlight]?.value);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      setOpen(false);
      setQuery('');
      inputRef.current?.blur();
    }
  };

  return (
    <div className="scope-picker" style={{ minWidth }} ref={wrapperRef}>
      <span className="scope-picker-label">{label}</span>
      <input
        ref={inputRef}
        type="text"
        className="scope-picker-input mono"
        disabled={disabled}
        placeholder={
          disabled ? (disabledPlaceholder ?? placeholder) : selected ? selected.label : placeholder
        }
        value={query}
        onChange={(e) => {
          setQuery(e.target.value);
          if (!open) setOpen(true);
        }}
        onFocus={() => {
          if (!disabled) setOpen(true);
        }}
        onKeyDown={onKeyDown}
      />
      {value && !disabled && (
        <button
          className="scope-picker-clear"
          type="button"
          onClick={() => commit(undefined)}
          title={`clear ${label} filter`}
        >
          x
        </button>
      )}
      {open && !disabled && (
        <div className="scope-picker-menu" ref={listRef}>
          {allowEmptyLabel && (
            <div
              className={`scope-picker-row ${value ? '' : 'selected'}`}
              onClick={() => commit(undefined)}
            >
              <span>{allowEmptyLabel}</span>
              <span className="scope-picker-meta">{options.length}</span>
            </div>
          )}
          {filtered.length === 0 ? (
            <div className="scope-picker-empty">no matches for "{query}"</div>
          ) : (
            filtered.map((option, i) => (
              <div
                key={option.value}
                data-scope-row={i}
                className={`scope-picker-row ${i === activeHighlight ? 'highlighted' : ''} ${
                  value === option.value ? 'selected' : ''
                }`}
                onClick={() => commit(option.value)}
                onMouseEnter={() => setHighlight(i)}
              >
                <span>{option.label}</span>
                <span className="scope-picker-meta">{option.meta}</span>
              </div>
            ))
          )}
        </div>
      )}
    </div>
  );
}

export function ProjectScopePicker({
  projects,
  value,
  onChange,
}: {
  projects: ProjectOption[];
  value: string | undefined;
  onChange: (project: string | undefined) => void;
}) {
  const options = useMemo(
    () =>
      projects.map((p) => ({
        value: p.label,
        label: p.label,
        meta: `${p.sessions} sessions · ${p.fragments} fragments`,
        searchText: `${p.label} ${p.sessions} ${p.fragments}`,
      })),
    [projects],
  );

  return (
    <ScopePicker
      label="folder"
      value={value}
      options={options}
      placeholder={`choose folder (${projects.length})`}
      onChange={onChange}
      minWidth={280}
    />
  );
}

export function SessionScopePicker({
  sessions,
  value,
  projectLabel,
  onChange,
}: {
  sessions: SessionListItem[];
  value: string | undefined;
  projectLabel: string | undefined;
  onChange: (session: string | undefined) => void;
}) {
  const options = useMemo(
    () =>
      sessions.map((s) => ({
        value: s.id,
        label: s.src_session_uuid || s.id,
        meta: `${basenameOf(s.project_cwd)} · ${s.fragment_count}`,
        searchText: `${s.id} ${s.src_session_uuid} ${basenameOf(s.project_cwd)} ${s.fragment_count}`,
      })),
    [sessions],
  );

  return (
    <ScopePicker
      label="session"
      value={value}
      options={options}
      placeholder={`all ${sessions.length} sessions`}
      disabled={!projectLabel}
      disabledPlaceholder="choose a folder first"
      allowEmptyLabel={`all ${sessions.length} sessions in ${projectLabel ?? 'folder'}`}
      onChange={onChange}
      minWidth={380}
    />
  );
}
