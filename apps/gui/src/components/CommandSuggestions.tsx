import { useEffect, useState } from 'react';

type Props = {
  options: string[];
  label: string;
  onChoose: (value: string) => void;
  onDismiss?: () => void;
};

export function CommandSuggestions({ options, label, onChoose, onDismiss }: Props) {
  const [index, setIndex] = useState(0);
  useEffect(() => setIndex(0), [options.join('\0')]);
  if (!options.length) return null;
  return <div className="slash-options" role="listbox" aria-label={label} onKeyDown={event => {
    if (event.key === 'ArrowDown') { event.preventDefault(); setIndex(value => (value + 1) % options.length); }
    if (event.key === 'ArrowUp') { event.preventDefault(); setIndex(value => (value + options.length - 1) % options.length); }
    if (event.key === 'Enter') { event.preventDefault(); onChoose(options[index]); }
    if (event.key === 'Escape') { event.preventDefault(); onDismiss?.(); }
  }}>{options.map((option, optionIndex) => <button type="button" role="option" aria-selected={optionIndex === index} key={option} onMouseDown={event => event.preventDefault()} onMouseEnter={() => setIndex(optionIndex)} onClick={() => onChoose(option)}>/{option}</button>)}</div>;
}
