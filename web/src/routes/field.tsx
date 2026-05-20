import { createFileRoute } from '@tanstack/react-router';

import { BrandMark, Icon } from '@/components/atoms';

export const Route = createFileRoute('/field')({
  component: FieldPage,
});

function FieldPage() {
  return (
    <div>
      <div className="page-header">
        <div>
          <h1 className="page-title">Field</h1>
          <div className="page-sub">
            Neural-field visualization · basin layout · attractor activation map
          </div>
        </div>
        <div className="page-actions">
          <span className="mono dim" style={{ fontSize: 11 }}>
            38 basins · 1,204 fragments
          </span>
        </div>
      </div>

      <div className="empty with-card">
        <BrandMark size={44} dim />
        <div className="empty-title">Field viz lands in the next port pass</div>
        <div className="empty-body">
          The d3-based attractor field — basin-of-attraction layout, activation glow, and
          decay-shading — is the most complex view in the design. It ships in a follow-up PR so this
          one can land cleanly. See <span className="mono">design/routes/field.jsx</span> for the
          working hi-fi prototype.
        </div>
        <a className="btn btn-ghost" style={{ marginTop: 2 }}>
          <Icon.Info /> design/screenshots/field.png
        </a>
      </div>
    </div>
  );
}
