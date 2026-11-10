/**
 * 新建工作流时的空白标准 BPMN 2.0 文档。
 *
 * 只包含一个可执行 Process 与空的 BPMNPlane，不预置任何私有扩展。
 */

export const BLANK_PROCESS_ID = 'Process_Workflow';

export const BLANK_BPMN_XML = `<?xml version="1.0" encoding="UTF-8"?>
<bpmn:definitions xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
                  xmlns:bpmn="http://www.omg.org/spec/BPMN/20100524/MODEL"
                  xmlns:bpmndi="http://www.omg.org/spec/BPMN/20100524/DI"
                  xmlns:dc="http://www.omg.org/spec/DD/20100524/DC"
                  xmlns:di="http://www.omg.org/spec/DD/20100524/DI"
                  id="Definitions_Workflow"
                  targetNamespace="http://assets-platform.local/bpmn">
  <bpmn:process id="${BLANK_PROCESS_ID}" name="新建工作流" isExecutable="true" />
  <bpmndi:BPMNDiagram id="BPMNDiagram_Workflow">
    <bpmndi:BPMNPlane id="BPMNPlane_Workflow" bpmnElement="${BLANK_PROCESS_ID}" />
  </bpmndi:BPMNDiagram>
</bpmn:definitions>
`;
