import type { PluginManifest } from '@omnistream/protocol';
export interface PluginContext { log(message: string, data?: Record<string, unknown>): void; requestApproval(action: string, payload: Record<string, unknown>): Promise<boolean>; }
export interface OmniStreamPlugin { manifest: PluginManifest; activate(context: PluginContext): Promise<void>; deactivate(): Promise<void>; }
export function validateManifest(manifest: PluginManifest): void {
  if (!manifest.id || !manifest.version || manifest.capabilities.length === 0) throw new Error('invalid plugin manifest');
}
