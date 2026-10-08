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
