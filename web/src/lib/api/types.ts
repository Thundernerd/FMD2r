import type { components } from './schema';

type Schemas = components['schemas'];

export type InboxItem = Schemas['InboxItem'];
export type TaskProgress = Schemas['TaskProgress'];
export type JobState = Schemas['JobState'];
export type LogLine = Schemas['LogLine'];
export type SeriesRef = Schemas['SeriesRef'];
export type LogLevel = Schemas['LogLevel'];
export type JobPhase = Schemas['JobPhase'];
export type About = Schemas['About'];
export type ToolCheck = Schemas['ToolCheck'];
export type Settings = Schemas['Settings'];
export type SaveToSettings = Schemas['SaveToSettings'];
export type RenamePreview = Schemas['RenamePreview'];
export type ModuleSummary = Schemas['ModuleSummary'];
export type ModuleSettingsView = Schemas['ModuleSettingsView'];
export type ModuleOptionSetting = Schemas['ModuleOptionSetting'];
export type Problem = Schemas['Problem'];
export type AccountInfo = Schemas['AccountInfo'];
export type AccountRequest = Schemas['AccountRequest'];
export type AccountState = Schemas['AccountState'];
