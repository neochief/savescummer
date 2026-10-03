import { fillArtwork, loadArtwork, makeGameLink } from './artwork.js';
import { galleryClasses } from './gallery-classes.js';
import { createCardReveal } from './reveal.js';

const { createPopupHug } = await import(/* @vite-ignore */ new URL('./character/popup-hug.js', document.baseURI).href);

const platform = navigator.userAgentData?.platform || navigator.platform || navigator.userAgent;
const mobile = navigator.userAgentData?.mobile || /android|iphone|ipad|ipod/i.test(navigator.userAgent)
  || (navigator.platform === "MacIntel" && navigator.maxTouchPoints > 1);
const label = mobile ? null
  : /win/i.test(platform) ? "Windows"
  : /mac/i.test(platform) ? "macOS"
  : /linux/i.test(platform) ? "Linux"
  : null;
if (label) document.getElementById("download-label").textContent = `Download for ${label}`;

const groups = window.savescummerGameGroups || [];
const gameGroups = document.getElementById("game-groups");
const showMore = document.getElementById("show-more");
const tooltip = document.getElementById("game-tooltip");
const tooltipArt = document.getElementById("game-tooltip-art");
const tooltipTitle = document.getElementById("game-tooltip-title");
const tooltipContext = document.getElementById("game-tooltip-context");
const tooltipValue = document.getElementById("game-tooltip-value");
const tooltipIcon = document.getElementById("game-tooltip-icon");
const tooltipCallout = document.getElementById("game-tooltip-callout-text");
const tooltipDetails = tooltip.querySelector(".game-tooltip-details");
const tooltipCopy = tooltip.querySelector(".game-tooltip-copy");
const mascot = createPopupHug(tooltip, tooltipCopy, positionTooltip);
const cardReveal = createCardReveal();
// The gallery's clipping also clips intersections until it is expanded.
const artworkObserver = new IntersectionObserver(entries => {
  for (const entry of entries) {
    if (!entry.isIntersecting) continue;
    loadArtwork(entry.target);
    artworkObserver.unobserve(entry.target);
  }
}, { rootMargin: '400px 0px' });
let activeCard = null;
let pinnedCard = null;
let hoveredCard = null;
let ignoredHoverCard = null;
let closeTimer = null;
let hoverCloseTimer = null;
let moveAnimation = null;

function stopMove() {
  if (moveAnimation) moveAnimation.cancel();
  moveAnimation = null;
}

function finishClose() {
  if (activeCard) return;
  window.clearTimeout(closeTimer);
  closeTimer = null;
  tooltip.hidden = true;
  tooltip.classList.remove("closing");
  mascot.update();
}

function deactivateCard() {
  if (!activeCard) return;
  activeCard.classList.remove("open");
  activeCard.querySelector(".game-card-trigger").setAttribute("aria-expanded", "false");
  const link = activeCard.querySelector(".game-link");
  link.tabIndex = -1;
  link.setAttribute("aria-hidden", "true");
  activeCard = null;
}

function hideTooltip() {
  window.clearTimeout(hoverCloseTimer);
  hoverCloseTimer = null;
  if (!activeCard) return;
  stopMove();
  deactivateCard();
  tooltip.classList.remove("settled");
  tooltip.classList.add("closing");
  if (window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) finishClose();
  else closeTimer = window.setTimeout(finishClose, 320);
}

function scheduleHoverClose() {
  window.clearTimeout(hoverCloseTimer);
  // Keep the popup open while the pointer crosses the gap to another card.
  hoverCloseTimer = window.setTimeout(() => {
    hoverCloseTimer = null;
    if (!pinnedCard && activeCard && hoveredCard !== activeCard && !activeCard.contains(document.activeElement)) hideTooltip();
  }, 180);
}

function showTooltip(card, game) {
  if (card.inert) return;
  cardReveal.show(card);
  loadArtwork(card);
  artworkObserver.unobserve(card);
  window.clearTimeout(hoverCloseTimer);
  hoverCloseTimer = null;
  window.clearTimeout(closeTimer);
  closeTimer = null;
  tooltip.classList.remove("closing");
  if (activeCard === card) return;
  const switchingCards = activeCard !== null;
  const shouldMove = switchingCards && tooltipCopy.animate && !window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
  const from = shouldMove ? tooltipCopy.getBoundingClientRect() : null;
  stopMove();
  tooltip.classList.toggle("settled", switchingCards);
  deactivateCard();
  activeCard = card;
  card.classList.add("open");
  card.querySelector(".game-card-trigger").setAttribute("aria-expanded", "true");
  const link = card.querySelector(".game-link");
  link.tabIndex = 0;
  link.removeAttribute("aria-hidden");
  fillArtwork(tooltipArt, game);
  tooltipArt.classList.remove("link-hover");
  tooltipArt.append(makeGameLink(game, true));
  tooltipTitle.textContent = game.name;
  tooltipContext.textContent = game.context;
  tooltipValue.textContent = game.value;
  tooltipDetails.scrollTop = 0;
  tooltipCallout.textContent = game.steam === 1145360 ? "My uncle Skelly is starring in Hades!"
    : game.steam === 1145350 ? "I wish I was as handsome as my uncle Skelly!" : "";
  tooltipIcon.style.backgroundImage = `url("./icons/value/${game.icon}.svg?v=fa-duotone-review-3")`;
  tooltip.classList.toggle("hugged", game.steam === 1145360 || game.steam === 1145350);
  tooltip.classList.remove("flip");
  tooltip.hidden = false;
  positionTooltip();
  if (!shouldMove) return;
  const to = tooltipCopy.getBoundingClientRect();
  const animation = tooltipCopy.animate([
    { transform: `translate(${from.left - to.left}px, ${from.top - to.top}px)` },
    { transform: "translate(0, 0)" },
  ], { duration: 160, easing: "cubic-bezier(.2,.7,.2,1)", fill: "both" });
  moveAnimation = animation;
  const finishMove = () => {
    if (moveAnimation === animation) stopMove();
  };
  animation.finished.then(finishMove, finishMove);
}

tooltipCopy.addEventListener("animationend", (event) => {
  if (event.target === tooltipCopy && tooltip.classList.contains("closing")) finishClose();
});

function pinTooltip(card, game) {
  if (pinnedCard) pinnedCard.classList.remove("pinned");
  pinnedCard = card;
  card.classList.add("pinned");
  tooltip.classList.add("pinned");
  ignoredHoverCard = null;
  showTooltip(card, game);
}

function unpinTooltip() {
  if (!pinnedCard) return;
  stopMove();
  ignoredHoverCard = hoveredCard === pinnedCard ? pinnedCard : null;
  pinnedCard.classList.remove("pinned");
  pinnedCard = null;
  tooltip.classList.remove("pinned");
  hideTooltip();
}

function positionTooltip() {
  if (!activeCard) return;
  const cardRect = activeCard.getBoundingClientRect();
  const cardFrame = parseFloat(getComputedStyle(activeCard).borderLeftWidth) || 0;
  tooltipArt.style.width = `${cardRect.width - cardFrame * 2}px`;
  tooltipArt.style.height = `${cardRect.height - cardFrame * 2}px`;
  const hugged = tooltip.classList.contains("hugged");
  tooltip.classList.remove("mirrored");
  if (hugged && window.innerWidth > 900) {
    const extent = mascot.bounds();
    const centeredLeft = cardRect.left + (cardRect.width - tooltip.offsetWidth) / 2;
    const roomLeft = centeredLeft - 12;
    const roomRight = window.innerWidth - centeredLeft - tooltip.offsetWidth - 12;
    tooltip.classList.toggle("mirrored", roomLeft < extent.left && roomRight > roomLeft);
  }
  tooltipCopy.style.removeProperty("--popup-details-height");
  tooltipDetails.removeAttribute("tabindex");
  if (hugged && window.innerWidth <= 900) {
    // Keep the speech bubble and hands visible; long descriptions can scroll on short screens.
    for (let pass = 0; pass < 3; pass++) {
      const extent = mascot.bounds();
      const first = Math.min(0, tooltipCopy.offsetTop - extent.top);
      const last = Math.max(tooltip.offsetHeight, tooltipCopy.offsetTop + tooltipCopy.offsetHeight + extent.bottom);
      const overflow = last - first - window.innerHeight + 24;
      if (overflow <= 0) break;
      tooltipCopy.style.setProperty("--popup-details-height", `${Math.max(88, tooltipDetails.offsetHeight - overflow)}px`);
      tooltipDetails.tabIndex = 0;
    }
  }
  const imageFrame = parseFloat(getComputedStyle(tooltipArt).borderLeftWidth) || 0;
  const tipWidth = tooltip.offsetWidth;
  const tipHeight = tooltip.offsetHeight;
  const flip = cardRect.top + tipHeight > window.innerHeight - 12;
  tooltip.classList.toggle("flip", flip);
  const top = flip ? cardRect.bottom - cardFrame + imageFrame - tipHeight : cardRect.top + cardFrame - imageFrame;
  const extent = hugged ? mascot.bounds() : { left: 0, right: 0, top: 0, bottom: 0 };
  const mascotLeft = extent.left;
  const mascotRight = extent.right;
  const left = Math.max(12 + mascotLeft, Math.min(cardRect.left + cardRect.width / 2 - tipWidth / 2, window.innerWidth - tipWidth - 12 - mascotRight));
  tooltip.style.top = `${Math.max(12, Math.min(top, window.innerHeight - tipHeight - 12))}px`;
  tooltip.style.left = `${left}px`;
  tooltipArt.style.marginLeft = `${cardRect.left - left + cardFrame - imageFrame}px`;
  if (hugged) {
    // Layout offsets stay stable while the copy unfolds or moves between cards.
    const minTop = Math.min(0, tooltipCopy.offsetTop - extent.top);
    const maxBottom = Math.max(tipHeight, tooltipCopy.offsetTop + tooltipCopy.offsetHeight + extent.bottom);
    tooltip.style.top = `${Math.max(12 - minTop, Math.min(top, window.innerHeight - 12 - maxBottom))}px`;
  }
  mascot.update();
}

function renderCards(grid, items) {
  const fragment = document.createDocumentFragment();
  for (const game of items) {
    const card = document.createElement("div");
    card.className = galleryClasses.card;
    const trigger = document.createElement("button");
    trigger.className = galleryClasses.trigger;
    trigger.type = "button";
    trigger.setAttribute("aria-label", `${game.name}. ${game.context} ${game.value}`);
    trigger.setAttribute("aria-expanded", "false");
    trigger.setAttribute("aria-controls", "game-tooltip");
    fillArtwork(trigger, game, { defer: true });
    const link = makeGameLink(game, false, hovered => tooltipArt.classList.toggle("link-hover", hovered));
    link.tabIndex = -1;
    link.setAttribute("aria-hidden", "true");
    card.append(trigger, link);
    trigger.addEventListener("focus", () => {
      if (!pinnedCard) showTooltip(card, game);
    });
    trigger.addEventListener("click", () => {
      if (pinnedCard === card) unpinTooltip();
      else pinTooltip(card, game);
    });
    card.addEventListener("pointerenter", () => {
      hoveredCard = card;
      if (!pinnedCard && ignoredHoverCard !== card) showTooltip(card, game);
    });
    card.addEventListener("pointerleave", () => {
      if (hoveredCard === card) hoveredCard = null;
      if (ignoredHoverCard === card) ignoredHoverCard = null;
      if (!pinnedCard && activeCard === card && !card.contains(document.activeElement)) scheduleHoverClose();
    });
    card.addEventListener("focusout", (event) => {
      if (!pinnedCard && hoveredCard !== card && activeCard === card && !card.contains(event.relatedTarget)) scheduleHoverClose();
    });
    fragment.append(card);
    artworkObserver.observe(card);
    cardReveal.observe(card);
  }
  grid.append(fragment);
}

for (const [index, group] of groups.entries()) {
  const section = document.createElement("section");
  section.className = galleryClasses.group;
  section.setAttribute("aria-labelledby", `game-group-title-${index}`);
  const header = document.createElement("div");
  header.className = galleryClasses.header;
  const title = document.createElement("h3");
  title.className = "m-0 text-center text-[1.17em] leading-[normal] font-bold";
  title.id = `game-group-title-${index}`;
  const label = document.createElement("span");
  label.className = galleryClasses.label;
  label.textContent = group.title;
  title.append(label);
  if (group.tagline) {
    const separator = document.createElement("span");
    separator.className = galleryClasses.separator;
    separator.textContent = "·";
    const tagline = document.createElement("span");
    tagline.className = galleryClasses.tagline;
    tagline.textContent = group.tagline;
    title.append(separator, tagline);
  }
  const grid = document.createElement("div");
  grid.className = galleryClasses.grid;
  header.append(title);
  section.append(header, grid);
  gameGroups.append(section);
  renderCards(grid, group.games);
}
showMore.hidden = gameGroups.scrollHeight <= gameGroups.clientHeight;
showMore.querySelector("span").textContent = `Show all ${groups.reduce((count, group) => count + group.games.length, 0)} games`;
showMore.addEventListener("click", () => {
  gameGroups.classList.add("expanded");
  updateGalleryAvailability();
  showMore.setAttribute("aria-expanded", "true");
  showMore.hidden = true;
});
document.addEventListener("click", (event) => {
  if (pinnedCard && !pinnedCard.contains(event.target) && !tooltipCopy.contains(event.target)) unpinTooltip();
});
window.addEventListener("scroll", positionTooltip, { passive: true });
window.addEventListener("resize", positionTooltip);
document.fonts.ready.then(positionTooltip);

// Keep fully faded and clipped cards out of keyboard navigation as well as pointer interaction.
function updateGalleryAvailability() {
  const collapsed = !gameGroups.classList.contains("expanded");
  const visiblePart = parseFloat(getComputedStyle(gameGroups).getPropertyValue("--gallery-visible-part")) / 100;
  const cutoff = gameGroups.getBoundingClientRect().top + gameGroups.clientHeight * visiblePart;
  for (const card of gameGroups.querySelectorAll(".game-card")) {
    card.inert = collapsed && card.getBoundingClientRect().top >= cutoff;
  }
  if (activeCard?.inert) {
    if (pinnedCard) unpinTooltip();
    else hideTooltip();
  }
}
new ResizeObserver(updateGalleryAvailability).observe(gameGroups);
document.fonts.ready.then(updateGalleryAvailability);
updateGalleryAvailability();

document.addEventListener("keydown", (event) => {
  if (event.key !== "Escape") return;
  const dismissedCard = pinnedCard || activeCard;
  const trigger = pinnedCard?.querySelector(".game-card-trigger");
  if (pinnedCard) unpinTooltip();
  else hideTooltip();
  // The popup can cover the pointer; closing it must not immediately reopen this card.
  ignoredHoverCard = dismissedCard;
  trigger?.focus({ preventScroll: true });
  hideTooltip();
});
