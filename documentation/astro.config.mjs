// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import xGrammar from './src/x.tmLanguage.json' with { type: 'json' };

// Kit documentation. Pages live in src/content/docs/; images in src/assets/.
// https://starlight.astro.build/reference/configuration/
export default defineConfig({
	integrations: [
		starlight({
			title: 'Kit',
			description: 'A modular 2D/3D game engine: Rust, Vulkan, ECS, physics, and the x scripting language.',
			logo: { src: './src/assets/icon.png' },
			favicon: '/favicon.png',
			social: [{ icon: 'github', label: 'GitHub', href: 'https://github.com/Caraveo/kit' }],
			editLink: { baseUrl: 'https://github.com/Caraveo/kit/edit/main/documentation/' },
			lastUpdated: true,
			customCss: ['./src/styles/kit.css'],
			// Syntax highlighting for x code blocks.
			expressiveCode: { shiki: { langs: [xGrammar] } },
			sidebar: [
				{ label: 'Welcome', link: '/' },
				{
					label: 'Getting started',
					items: ['getting-started/install', 'getting-started/first-game', 'getting-started/concepts'],
				},
				{
					label: 'Editor',
					items: [
						'editor/tour',
						'editor/objects-and-components',
						'editor/scenes-and-assets',
						'editor/play-mode',
						'editor/canvas-mode',
						'editor/terrain',
						'editor/sound',
						'editor/post-processing',
						'editor/diagrams',
						'editor/rigg',
						'editor/rex',
						'editor/export',
					],
				},
				{
					label: 'Scripting with x',
					items: [
						'scripting/basics',
						'scripting/classes',
						'scripting/attaching-scripts',
						'scripting/engine-modules',
						'scripting/recipes',
					],
				},
				{
					label: 'Physics',
					items: ['physics/colliders', 'physics/physics-bodies', 'physics/settings', 'physics/scripting-physics'],
				},
				{
					label: 'Projects',
					items: ['projects/project-structure', 'projects/project-settings', 'projects/file-formats'],
				},
				{
					label: 'Engine',
					items: ['engine/architecture', 'engine/rust-api', 'engine/build-from-source'],
				},
				{
					label: 'Reference',
					items: [
						'reference/keyboard-and-mouse',
						'reference/x-language',
						'reference/troubleshooting',
						'reference/changelog',
					],
				},
			],
		}),
	],
});
