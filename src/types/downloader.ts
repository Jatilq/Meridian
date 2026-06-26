// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

export type DownloadItem = {
  id: string;
  url: string;
  status: 'pending' | 'downloading' | 'paused' | 'completed' | 'failed' | 'cancelled';
  progress: number;
  total_bytes: number | null;
  downloaded_bytes: number;
  file_path: string | null;
  file_name: string;
  created_at: number;
  finished_at: number | null;
  error: string | null;
};

export type DownloadStatus = DownloadItem['status'];
