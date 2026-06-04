export interface Paper {
  kind: string;
  source: string;
  topic: string;
  title: string;
  link: string;
  date_label: string;
  ts: number;
  summary: string;
  grounding: string;
}

export interface SourceError { source: string; message: string; }
export interface Feed { papers: Paper[]; errors: SourceError[]; }
