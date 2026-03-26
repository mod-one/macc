import React from 'react';
import Markdown from 'react-markdown';
import { useSearchParams } from 'react-router-dom';
import { LightAsync as SyntaxHighlighter } from 'react-syntax-highlighter';
import { github } from 'react-syntax-highlighter/dist/esm/styles/hljs';
import { HELP_SECTIONS, getHelpSectionById, type HelpSectionId } from './helpDocs';

function normalizeSearchValue(value: string): string {
  return value.trim().toLowerCase();
}

const Help: React.FC = () => {
  const [searchParams, setSearchParams] = useSearchParams();
  const urlSection = getHelpSectionById(searchParams.get('section'));
  const [search, setSearch] = React.useState('');
  const [selectedSectionId, setSelectedSectionId] = React.useState<HelpSectionId>(
    urlSection?.id ?? HELP_SECTIONS[0].id,
  );

  React.useEffect(() => {
    if (!urlSection) {
      return;
    }

    setSelectedSectionId(urlSection.id);
  }, [urlSection]);

  const normalizedSearch = normalizeSearchValue(search);
  const filteredSections = React.useMemo(
    () =>
      HELP_SECTIONS.filter((section) => {
        if (!normalizedSearch) {
          return true;
        }

        return normalizeSearchValue(`${section.title}\n${section.content}`).includes(normalizedSearch);
      }),
    [normalizedSearch],
  );

  const activeSection = filteredSections.find((section) => section.id === selectedSectionId) ?? filteredSections[0];
  const activeSectionId = activeSection?.id;

  React.useEffect(() => {
    if (!activeSection) {
      return;
    }

    if (activeSection.id === selectedSectionId) {
      return;
    }

    setSelectedSectionId(activeSection.id);
  }, [activeSection, selectedSectionId]);

  const onSelectSection = React.useCallback(
    (sectionId: HelpSectionId) => {
      setSelectedSectionId(sectionId);
      setSearchParams(
        (prev) => {
          const next = new URLSearchParams(prev);
          next.set('section', sectionId);
          return next;
        },
        { replace: true },
      );
    },
    [setSearchParams],
  );

  return (
    <div className="grid gap-6 lg:grid-cols-[260px_minmax(0,1fr)]">
      <aside className="space-y-4 rounded-2xl border border-[var(--border)] bg-[var(--bg-secondary)] p-4">
        <header className="space-y-1">
          <h1 className="text-lg font-semibold text-[var(--text-primary)]">Help & Docs</h1>
          <p className="text-sm text-[var(--text-secondary)]">Search and browse bundled web UI documentation.</p>
        </header>

        <input
          aria-label="Search help docs"
          className="w-full rounded-md border border-[var(--border)] bg-[var(--bg-primary)] px-3 py-2 text-sm text-[var(--text-primary)] outline-none ring-0 placeholder:text-[var(--text-muted)] focus:border-[var(--accent)]"
          onChange={(event) => setSearch(event.target.value)}
          placeholder="Search docs..."
          type="search"
          value={search}
        />

        <nav aria-label="Help sections">
          <ul className="space-y-1">
            {filteredSections.map((section) => {
              const isActive = section.id === activeSectionId;
              return (
                <li key={section.id}>
                  <button
                    aria-current={isActive ? 'true' : undefined}
                    className={`w-full rounded-md px-3 py-2 text-left text-sm transition-colors ${
                      isActive
                        ? 'bg-[var(--accent)]/15 text-[var(--text-primary)]'
                        : 'text-[var(--text-secondary)] hover:bg-[var(--bg-card)]'
                    }`}
                    onClick={() => onSelectSection(section.id)}
                    type="button"
                  >
                    {section.title}
                  </button>
                </li>
              );
            })}
          </ul>
        </nav>

        <p className="text-xs text-[var(--text-muted)]">
          Need deeper docs? Open <code>README.md</code> and <code>MACC.md</code> in the repository root.
        </p>
      </aside>

      <section className="min-w-0 rounded-2xl border border-[var(--border)] bg-[var(--bg-secondary)] p-6">
        {activeSection ? (
          <article className="prose prose-slate max-w-none prose-code:text-[var(--text-primary)] prose-pre:border prose-pre:border-[var(--border)] prose-pre:bg-[var(--bg-card)] prose-p:text-[var(--text-secondary)] prose-headings:text-[var(--text-primary)]">
            <Markdown
              components={{
                code(props) {
                  const { children, className, ...rest } = props;
                  const codeText = String(children ?? '');
                  const match = /language-(\w+)/.exec(className || '');

                  if (!match) {
                    return (
                      <code className={className} {...rest}>
                        {children}
                      </code>
                    );
                  }

                  return (
                    <SyntaxHighlighter
                      customStyle={{ borderRadius: '0.5rem', margin: '1rem 0' }}
                      language={match[1]}
                      style={github}
                    >
                      {codeText.replace(/\n$/, '')}
                    </SyntaxHighlighter>
                  );
                },
              }}
            >
              {activeSection.content}
            </Markdown>
          </article>
        ) : (
          <div className="rounded-lg border border-dashed border-[var(--border)] p-6 text-sm text-[var(--text-muted)]">
            No sections match your search.
          </div>
        )}
      </section>
    </div>
  );
};

export default Help;
