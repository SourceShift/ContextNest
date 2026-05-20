import { Logo } from './Logo';

type Props = {
  title: string;
  body?: string;
  action?: React.ReactNode;
};

export function EmptyState({ title, body, action }: Props) {
  return (
    <div className="card flex flex-col items-center justify-center gap-4 py-16 text-center">
      <Logo className="size-10 text-[--color-ink-dim] opacity-60" />
      <div className="flex max-w-md flex-col gap-1">
        <div className="text-base font-medium text-[--color-ink]">{title}</div>
        {body ? (
          <div className="text-sm text-[--color-ink-muted]">{body}</div>
        ) : null}
      </div>
      {action ? <div>{action}</div> : null}
    </div>
  );
}
