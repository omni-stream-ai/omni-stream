import { useEffect, useRef, useState } from 'react';
import { ChevronLeft, ChevronRight, Maximize, Minus, Plus, X } from 'lucide-react';
import { useI18n } from '../i18n';

export type PreviewImage = { url: string; name: string };

export function ImagePreview({ images, initial = 0, onClose }: { images: PreviewImage[]; initial?: number; onClose: () => void }) {
  const { t } = useI18n();
  const [index, setIndex] = useState(initial);
  const [zoom, setZoom] = useState(1);
  const [offset, setOffset] = useState({ x: 0, y: 0 });
  const drag = useRef<{ x: number; y: number } | null>(null);
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => { dialog.current?.showModal(); }, []);
  const change = (next: number) => { setIndex(next); setZoom(1); setOffset({ x: 0, y: 0 }); };
  const scale = (amount: number) => setZoom(value => Math.max(0.25, Math.min(8, value * amount)));
  return <dialog ref={dialog} className="image-preview" aria-label={t('image.preview')} onCancel={onClose} onKeyDown={event => {
    if (event.key === 'Enter' && !(event.target instanceof HTMLButtonElement)) { event.preventDefault(); onClose(); }
    const delta: Record<string, [number, number]> = { ArrowLeft: [-30, 0], ArrowRight: [30, 0], ArrowUp: [0, -30], ArrowDown: [0, 30] };
    if (delta[event.key]) { event.preventDefault(); const [x, y] = delta[event.key]; setOffset(value => ({ x: value.x + x, y: value.y + y })); }
  }}>
    <header><span>{images[index]?.name}</span><div>
      <button title={t('image.zoomOut')} aria-label={t('image.zoomOut')} onClick={() => scale(0.8)}><Minus size={18} /></button>
      <button title={t('image.reset')} aria-label={t('image.reset')} onClick={() => { setZoom(1); setOffset({ x: 0, y: 0 }); }}><Maximize size={18} /></button>
      <button title={t('image.zoomIn')} aria-label={t('image.zoomIn')} onClick={() => scale(1.25)}><Plus size={18} /></button>
      <button title={t('image.close')} aria-label={t('image.close')} onClick={onClose}><X size={20} /></button>
    </div></header>
    <div className="image-stage" onWheel={event => scale(event.deltaY > 0 ? 0.9 : 1.1)} onPointerDown={event => {
      drag.current = { x: event.clientX, y: event.clientY }; event.currentTarget.setPointerCapture(event.pointerId);
    }} onPointerMove={event => { if (!drag.current) return; const dx = event.clientX - drag.current.x, dy = event.clientY - drag.current.y; setOffset(value => ({ x: value.x + dx, y: value.y + dy })); drag.current = { x: event.clientX, y: event.clientY }; }} onPointerUp={() => { drag.current = null; }} onPointerCancel={() => { drag.current = null; }}>
      <img src={images[index]?.url} alt={images[index]?.name} draggable={false} style={{ transform: `translate(${offset.x}px, ${offset.y}px) scale(${zoom})` }} />
    </div>
    {index > 0 && <button className="preview-previous" title={t('image.previous')} aria-label={t('image.previous')} onClick={() => change(index - 1)}><ChevronLeft /></button>}
    {index < images.length - 1 && <button className="preview-next" title={t('image.next')} aria-label={t('image.next')} onClick={() => change(index + 1)}><ChevronRight /></button>}
    {images.length > 1 && <footer>{images.map((image, i) => <button key={`${image.url}-${i}`} aria-label={t('image.view', { index: i + 1 })} aria-pressed={i === index} onClick={() => change(i)}><img src={image.url} alt={image.name} /></button>)}</footer>}
  </dialog>;
}
