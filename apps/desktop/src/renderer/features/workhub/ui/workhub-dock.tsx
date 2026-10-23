/*
 * Licensed to the Apache Software Foundation (ASF) under one
 * or more contributor license agreements.  See the NOTICE file
 * distributed with this work for additional information
 * regarding copyright ownership.  The ASF licenses this file
 * to you under the Apache License, Version 2.0 (the
 * "License"); you may not use this file except in compliance
 * with the License.  You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing,
 * software distributed under the License is distributed on an
 * "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
 * KIND, either express or implied.  See the License for the
 * specific language governing permissions and limitations
 * under the License.
 */

import { isNativeSurfaceOccluded } from '../../../application/contracts/native-surface-occlusion.js';
import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { Button } from '@astryxdesign/core';
import { useUiLocale } from '@maka/ui';
import type { WorkHubPresentationSnapshot } from '../../../../shared/workhub-presentation.js';
import { useWorkHubServices } from '../services.js';
import { workHubLiveCopy } from '../locales/workhub-live-copy.js';

/** Occlusion/menus can move without resizing the dock; poll while docked, not every frame. */
const DOCK_HOST_POLL_MS = 250;

/** The main window owns only this landing space; the live view keeps its React owner. */
export function WorkHubDock({ enabled, visible = true, workbarCollapsed }: { enabled: boolean; visible?: boolean; workbarCollapsed: boolean }) {
  const { presentation } = useWorkHubServices();
  const t = workHubLiveCopy[useUiLocale()];
  const element = useRef<HTMLElement>(null);
  const collapsed = useRef(workbarCollapsed);
  collapsed.current = workbarCollapsed;
  const [snapshot, setSnapshot] = useState<WorkHubPresentationSnapshot>();
  const [backdrop, setBackdrop] = useState<string>();
  const [error, setError] = useState<string>();
  const needsRecovery = snapshot?.placement === 'docked' && snapshot.rendererCrashed;
  const report = (reason: unknown) =>
    setError(reason instanceof Error ? reason.message : String(reason));
  useEffect(() => {
    let active = true;
    const update = (next: WorkHubPresentationSnapshot) => {
      if (active) setSnapshot(next);
    };
    const unsubscribe = presentation.subscribe(update);
    void presentation.getSnapshot().then(update).catch(report);
    return () => {
      active = false;
      unsubscribe();
    };
  }, [presentation]);
  useLayoutEffect(() => {
    const node = element.current;
    if (!node) return;
    let active = true;
    let revision = 0;
    let last = '';
    let covered = false;
    let lastBackdrop: string | undefined;
    let pollTimer = 0;
    let coalesceFrame = 0;

    const publish = () => {
      if (!active) return;
      const raw = node.getBoundingClientRect();
      // Integer geometry only — subpixel jitter otherwise changes the key every
      // tick, storms setHost/setBackdrop, and pins the renderer at ~100% CPU.
      const rect = {
        x: Math.round(raw.x),
        y: Math.round(raw.y),
        width: Math.round(raw.width),
        height: Math.round(raw.height),
      };
      const docked = snapshot?.placement === 'docked';
      const occluded = visible && docked && isNativeSurfaceOccluded(rect, node.ownerDocument);
      const host = {
        visible: enabled && visible && rect.width > 0 && rect.height > 0,
        occluded,
        workbar: {
          collapsed: collapsed.current,
          placement: window.matchMedia('(max-width: 990px)').matches ? ('bottom' as const) : ('right' as const),
        },
        rect,
      };
      const key = JSON.stringify(host);
      if (key === last) return;
      last = key;
      if (covered !== occluded) ++revision;
      covered = occluded;
      const current = revision;
      if (!occluded && lastBackdrop !== undefined) {
        lastBackdrop = undefined;
        setBackdrop(undefined);
      }
      void presentation
        .setHost(host)
        .then((image) => {
          if (!active || current !== revision) return;
          if (!image || image === lastBackdrop) return;
          lastBackdrop = image;
          setBackdrop(image);
        })
        .catch(report);
    };

    publish();

    const docked = snapshot?.placement === 'docked';
    if (!(visible && docked)) {
      return () => {
        active = false;
        void presentation
          .setHost({ visible: false, rect: { x: 0, y: 0, width: 0, height: 0 } })
          .catch(() => undefined);
      };
    }

    // Event-driven geometry + low-rate occlusion poll. A perpetual rAF here
    // starved the Guest main thread under Agnx embed (click freeze / no caret).
    let publishCount = 0;
    const publishLogged = () => {
      publishCount += 1;
      if (publishCount <= 3 || publishCount % 20 === 0) {
        console.warn(
          `FREEZE_CHAIN|${JSON.stringify({
            phase: 'G.workhub.publish',
            data: { n: publishCount, placement: snapshot?.placement ?? null },
          })}`,
        );
      }
      publish();
    };
    const scheduleLogged = () => {
      if (coalesceFrame) return;
      coalesceFrame = window.requestAnimationFrame(() => {
        coalesceFrame = 0;
        publishLogged();
      });
    };
    const resizeObserver = typeof ResizeObserver === 'function' ? new ResizeObserver(() => scheduleLogged()) : null;
    resizeObserver?.observe(node);
    const onWindowChange = () => scheduleLogged();
    window.addEventListener('resize', onWindowChange);
    // Capture phase: sidebar / ancestor scrolls move the dock without resizing it.
    document.addEventListener('scroll', onWindowChange, true);
    const mutationObserver =
      typeof MutationObserver === 'function'
        ? new MutationObserver(() => scheduleLogged())
        : null;
    // Attributes only — childList would fire on every React commit.
    mutationObserver?.observe(document.documentElement, {
      subtree: true,
      attributes: true,
      attributeFilter: ['open', 'data-state', 'popover', 'aria-expanded'],
    });
    pollTimer = window.setInterval(publishLogged, DOCK_HOST_POLL_MS);

    return () => {
      active = false;
      resizeObserver?.disconnect();
      mutationObserver?.disconnect();
      window.removeEventListener('resize', onWindowChange);
      document.removeEventListener('scroll', onWindowChange, true);
      if (coalesceFrame) window.cancelAnimationFrame(coalesceFrame);
      if (pollTimer) window.clearInterval(pollTimer);
      void presentation
        .setHost({ visible: false, rect: { x: 0, y: 0, width: 0, height: 0 } })
        .catch(() => undefined);
    };
  }, [enabled, presentation, visible, snapshot?.placement]);
  return (
    <section
      ref={element}
      className="workHubDock"
      data-native-edge={(snapshot?.placement === 'docked' && !needsRecovery) || undefined}
      hidden={!visible}
      aria-label={t.title}
    >
      {backdrop && snapshot?.placement === 'docked' && (
        <img className="workHubDockBackdrop" src={backdrop} alt="" aria-hidden draggable={false} />
      )}
      {(snapshot?.placement === 'floating' || needsRecovery) && (
        <div className="workHubDockPlaceholder">
          <h2>{needsRecovery ? t.reloadRequired : t.floating}</h2>
          <Button
            label={needsRecovery ? t.retry : t.restore}
            onClick={() => {
              setError(undefined);
              void presentation.dock().catch(report);
            }}
          />
        </div>
      )}
      {error && <p role="alert">{error}</p>}
    </section>
  );
}
