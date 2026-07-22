export type Metric = {
  label: string;
  value: string;
  unit: string;
  trend: string;
  trendDirection: 'up' | 'down' | 'steady';
  hint: string;
};

export type NightSummary = {
  date: string;
  day: string;
  duration: string;
  ahi: number;
  leak: number;
  recording: 'Detailed' | 'Summary only';
};

export const overviewMetrics: Metric[] = [
  {
    label: 'Usage',
    value: '7h 42m',
    unit: 'last night',
    trend: 'Sample comparison: +24 min',
    trendDirection: 'up',
    hint: 'Time receiving therapy',
  },
  {
    label: 'AHI',
    value: '1.8',
    unit: 'events / hour',
    trend: 'Sample comparison: −0.4',
    trendDirection: 'down',
    hint: 'Apnea-hypopnea index',
  },
  {
    label: 'Pressure',
    value: '10.4',
    unit: 'cmH₂O at 95%',
    trend: 'Sample seven-night median',
    trendDirection: 'steady',
    hint: '95th percentile pressure',
  },
  {
    label: 'Leak',
    value: '4.2',
    unit: 'L/min at 95%',
    trend: 'Sample comparison: −1.2',
    trendDirection: 'down',
    hint: '95th percentile unintentional leak',
  },
];

export const recentNights: NightSummary[] = [
  { date: '21 Jul', day: 'Tuesday', duration: '7h 42m', ahi: 1.8, leak: 4.2, recording: 'Detailed' },
  { date: '20 Jul', day: 'Monday', duration: '7h 18m', ahi: 2.1, leak: 5.6, recording: 'Detailed' },
  { date: '19 Jul', day: 'Sunday', duration: '8h 06m', ahi: 1.4, leak: 3.8, recording: 'Detailed' },
  { date: '18 Jul', day: 'Saturday', duration: '6h 54m', ahi: 2.7, leak: 7.1, recording: 'Summary only' },
  { date: '17 Jul', day: 'Friday', duration: '7h 31m', ahi: 3.2, leak: 6.4, recording: 'Detailed' },
];

export const trendValues = [
  { label: '15 Jul', usage: 7.2, ahi: 2.9 },
  { label: '16 Jul', usage: 7.6, ahi: 2.4 },
  { label: '17 Jul', usage: 7.5, ahi: 3.2 },
  { label: '18 Jul', usage: 6.9, ahi: 2.7 },
  { label: '19 Jul', usage: 8.1, ahi: 1.4 },
  { label: '20 Jul', usage: 7.3, ahi: 2.1 },
  { label: '21 Jul', usage: 7.7, ahi: 1.8 },
];

export const dailyMetrics: Metric[] = [
  {
    label: 'AHI',
    value: '1.8',
    unit: 'events / hour',
    trend: 'Fabricated nightly value',
    trendDirection: 'down',
    hint: 'Apnea-hypopnea index',
  },
  {
    label: 'Therapy usage',
    value: '7h 42m',
    unit: 'sample session duration',
    trend: 'Sample: one session',
    trendDirection: 'steady',
    hint: 'Recorded therapy session',
  },
  {
    label: 'Pressure',
    value: '10.4',
    unit: 'cmH₂O at 95%',
    trend: 'Sample median 8.7',
    trendDirection: 'steady',
    hint: 'Therapy pressure',
  },
  {
    label: 'Leak',
    value: '4.2',
    unit: 'L/min at 95%',
    trend: 'Fabricated 95th percentile',
    trendDirection: 'down',
    hint: 'Unintentional mask leak',
  },
];

export const eventFlags = [
  { type: 'CA', label: 'Clear airway', time: '23:53', left: 14, width: 1.6, color: '#6f66c7' },
  { type: 'H', label: 'Hypopnea', time: '00:51', left: 26, width: 2.2, color: '#d27837' },
  { type: 'OA', label: 'Obstructive apnea', time: '02:19', left: 46, width: 1.8, color: '#ce526c' },
  { type: 'H', label: 'Hypopnea', time: '03:34', left: 62, width: 2.8, color: '#d27837' },
  { type: 'CA', label: 'Clear airway', time: '04:53', left: 79, width: 1.6, color: '#6f66c7' },
];

export const waveformPaths = {
  pressure:
    'M0 83 C40 80 65 72 100 66 S165 44 220 48 S310 32 360 42 S430 31 480 36 S545 24 600 28 S685 34 760 21 S830 19 900 23',
  flow:
    'M0 66 C18 36 30 98 48 62 S78 24 96 70 S126 103 144 59 S176 21 194 65 S225 104 244 60 S274 18 294 64 S326 101 346 57 S378 22 398 65 S430 107 450 58 S481 18 502 63 S534 101 556 56 S588 24 610 64 S642 104 664 58 S696 21 718 63 S751 99 774 55 S807 25 830 64 S864 100 900 58',
  leak:
    'M0 91 C60 91 82 87 126 88 S198 75 248 79 S325 85 378 72 S447 69 503 76 S590 62 650 69 S735 64 790 72 S850 59 900 62',
};
