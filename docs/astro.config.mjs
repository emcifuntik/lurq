import {defineConfig} from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
    site: 'https://ivanzaida.github.io',
    base: '/lurq',
    integrations: [
        starlight({
            title: 'lurq',
            description: 'Documentation for the lurq Rust UI toolkit.',
            sidebar: [
                {
                    label: 'Guides',
                    items: [
                        'getting-started',
                        'mental-model',
                        'components',
                        'reactivity',
                        'layout',
                        'theme',
                        'animation-transforms',
                        'styling-events',
                        'focus-navigation',
                        'forms',
                        'routing',
                        'futures-timers',
                        'queries',
                        'persistent-storage',
                        'i18n',
                        'modals',
                        'app-runtime',
                        'window-lifecycle-menus',
                        'ios',
                        'devtools',
                        'mcp',
                        'resources-media',
                        'canvas',
                        'testing',
                        'text-pipeline-optimization',
                    ],
                },
                {
                    label: 'Reference',
                    items: [
                        'ctx',
                        'dsl',
                        'retained_nodes',
                    ],
                },
            ],
        }),
    ],
});
