type Schema = {
  type?: string;
  title?: string;
  description?: string;
  enum?: (string | number | boolean)[];
  properties?: Record<string, Schema>;
  required?: string[];
  items?: Schema;
  minimum?: number;
  maximum?: number;
};

import { useI18n } from '../i18n';

export function SchemaFields({ schema, value, onChange, prefix = '' }: { schema: Schema; value: Record<string, unknown>; onChange: (value: Record<string, unknown>) => void; prefix?: string }) {
  const { t } = useI18n();
  return <>{Object.entries(schema.properties ?? {}).map(([key, field]) => {
    const label = field.title || key;
    const update = (next: unknown) => onChange({ ...value, [key]: next });
    if (field.type === 'object') return <fieldset key={key}><legend>{label}</legend><SchemaFields schema={field} value={(value[key] as Record<string, unknown>) ?? {}} onChange={update} prefix={`${prefix}${key}.`} /></fieldset>;
    if (field.type === 'boolean') return <label className="boolean-field" key={key}><input type="checkbox" checked={value[key] === true} onChange={event => update(event.target.checked)} />{label}</label>;
    return <label key={key}>{label}{schema.required?.includes(key) ? ' *' : ''}
      {field.enum ? <select aria-label={`${prefix}${label}`} value={String(value[key] ?? '')} onChange={event => update(field.enum?.find(option => String(option) === event.target.value))}><option value="">{t('approval.select')}</option>{field.enum.map(option => <option key={String(option)} value={String(option)}>{String(option)}</option>)}</select>
      : field.type === 'array' && field.items?.enum ? <select multiple aria-label={`${prefix}${label}`} value={(value[key] as string[]) ?? []} onChange={event => update([...event.target.selectedOptions].map(option => option.value))}>{field.items.enum.map(option => <option key={String(option)}>{String(option)}</option>)}</select>
      : field.type === 'array' ? <textarea aria-label={`${prefix}${label}`} value={((value[key] as string[]) ?? []).join('\n')} onChange={event => update(event.target.value.split('\n'))} />
      : <input aria-label={`${prefix}${label}`} type={['number', 'integer'].includes(field.type ?? '') ? 'number' : 'text'} step={field.type === 'integer' ? 1 : 'any'} min={field.minimum} max={field.maximum} required={schema.required?.includes(key)} value={String(value[key] ?? '')} onChange={event => update(['number', 'integer'].includes(field.type ?? '') && event.target.value !== '' ? Number(event.target.value) : event.target.value)} />}
      {field.description && <small>{field.description}</small>}
    </label>;
  })}</>;
}
